import { NO_BRANCH_LABEL } from "../../features/source-control/model/worktrees";
import {
  type WorktreeFocus,
  inWorktreeFocus,
  useWorktreeFocus,
} from "../../features/source-control/model/worktreeFocus";
import {
  SidebarWorktreeSwitcher,
  type ProjectMenuFlyout,
} from "../../features/source-control/ui/SidebarWorktreeSwitcher";
import {
  CurrentProjectIcon,
  useProjectLabel,
} from "../../features/projects/ui/CurrentProjectIcon";
import { OrchestrationSidebarAgents } from "../../features/orchestration/ui/OrchestrationSidebarAgents";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  Archive,
  Chatting,
  Check,
  ChevronDown,
  ChevronLeft,
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
  type IconComponent,
} from "../../shared/ui/icons";
import { createPortal } from "react-dom";
import { useProjectSwipe } from "./useProjectSwipe";
import {
  Fragment,
  memo,
  useCallback,
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
  loadRightPanelTabOrder,
  saveRightPanelTabOrder,
  type RightPanelTab,
  type SidebarTabId,
} from "../../features/settings/model/appearance";
import { formatInteger } from "../../shared/lib/numbers";
import {
  type GitFileDiffKind,
  type GitHistoryCommit,
} from "../../platform/tauri/fs";
import { MOD, SHIFT } from "../../platform/tauri/platform";
import { useTrafficLightsVisible } from "../../platform/tauri/useTrafficLightsVisible";
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
import { LAYER } from "../../shared/lib/layers";
import { RevealHeight } from "../../shared/ui/RevealHeight";
import { MODE_COMMAND_STYLES } from "../../features/sessions/ui/modeCommands";
import { OPERATOR_COMMAND } from "../../features/sessions/model/operatorCommand";
import { ORCHESTRATOR_COMMAND } from "../../features/sessions/model/orchestratorCommand";
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
import {
  keybindingShortcutLabel,
  subscribeKeybindings,
  type SettingsSectionId,
} from "../../features/settings/model/settings";
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
  loadProjectRailOrder,
  looksLikeProject,
  projectRailItems,
  saveProjectRailOrder,
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
import { SessionCardPreview } from "../../features/sessions/ui/SessionCardPreview";
import { LiveAgentsPreview } from "../../features/sessions/ui/LiveAgentsPreview";
import { ProjectRail } from "./ProjectRail";
import { InboxNotificationMenu } from "../../features/inbox/ui/InboxNotificationMenu";
import { RailAction, RailSearch } from "./RailAction";
import { TerminalSpinner } from "../../features/sessions/ui/TerminalSpinner";
import { DevModeSlot, IconButton, TabVisitNav } from "./TitleBar";
import { useT } from "../../shared/hooks/useI18n";
import { ProjectSearch } from "../../features/projects/ui/ProjectSearch";
import { Popover } from "../../shared/ui/Popover";
import {
  pickerProjects,
  ProjectPickerPopover,
  SearchableProjectPicker,
} from "../../features/projects/ui/SearchableProjectPicker";
import { useRailTooltip } from "./RailTooltip";
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
import { GitHistoryGraph } from "../../features/source-control/ui/GitHistoryGraph";
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
  MonoRailSection,
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
import {
  parseRemotePath,
  remotePath,
  remoteProjectFor,
} from "../../features/connections/model/remoteProjects";

const MIN_WIDTH = 260;
/** The inset sidebar's one-line rows stay legible narrower. */
const INSET_MIN_WIDTH = 200;
/** How close to the window's left edge the pointer peeks the sidebar out. */
const PEEK_EDGE = 6;
/** Grace before a peek closes, so the pointer can cross a gap to the panel. */
const PEEK_CLOSE_DELAY = 180;
/** An open menu, not a hover preview, which keeps a peek out. */
const PEEK_HOLDING_MENU =
  "[data-popover-side]:not([role=tooltip]):not(:has([role=tooltip]))";
const MAX_WIDTH = 560;
const DEFAULT_WIDTH = 260;
const REMINDERS_COLOR = "#f59e0b";

let rememberedWidth = DEFAULT_WIDTH;

type SidebarTab = SidebarTabId;

const TAB_LABELS: Record<SidebarTab | RightPanelTab, string> = {
  sessions: "Sessions",
  inbox: "Inbox",
  files: "Explorer",
  changes: "Changes",
  graph: "History",
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
  /**
   * The inset layout sets the sidebar on the window's outer glass, beside its
   * card. Its header menu switches projects, so the rail only shows to carry
   * the settings navigation.
   */
  inset?: boolean;
  /**
   * Inset layout: Explorer, Changes and Graph render into this panel beside the
   * card, and the sidebar keeps to the sessions.
   */
  rightPanel?: {
    host: HTMLElement | null;
    open: boolean;
    tab: RightPanelTab;
    onTabChange: (tab: RightPanelTab) => void;
  };
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
  inset = false,
  rightPanel,
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
  const t = useT();
  const trafficLightsVisible = useTrafficLightsVisible();
  const remoteProject = isRemoteProjectPath(cwd);
  const tab: SidebarTabId = requestedTab;
  const remote = useRemoteProjectSessions(cwd, remoteProject);
  const hostProject = remoteProject ? remoteProjectFor(cwd) : undefined;
  const remoteChange = async (
    sessionId: string,
    patch: {
      title?: string;
      archived?: boolean;
      pinned?: boolean;
      linkedWorkItem?: LinkedWorkItem | null;
    },
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
    if (
      !window.confirm(
        `Delete ${sessionIds.length === 1 ? "this conversation" : `${sessionIds.length} conversations`}? This can’t be undone.`,
      )
    )
      return;
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
  const onPlaceSessionOnPane = remoteProject
    ? undefined
    : onPlaceLocalSessionOnPane;
  const onRenameSession = remoteProject
    ? (sessionId: string, title: string) => {
        void remoteChange(sessionId, { title });
      }
    : onRenameLocalSession;
  const onArchiveSession = remoteProject
    ? (sessionId: string, archived: boolean) => {
        void remoteChange(sessionId, { archived });
      }
    : onArchiveLocalSession;
  const onArchiveSessions = remoteProject
    ? (sessionIds: readonly string[], archived: boolean) => {
        void Promise.all(
          sessionIds.map((id) => remoteChange(id, { archived })),
        );
      }
    : onArchiveLocalSessions;
  const onPinSession = remoteProject
    ? (sessionId: string, pinned: boolean) => {
        void remoteChange(sessionId, { pinned });
      }
    : onPinLocalSession;
  const onPinSessions = remoteProject
    ? (sessionIds: readonly string[], pinned: boolean) => {
        void Promise.all(sessionIds.map((id) => remoteChange(id, { pinned })));
      }
    : onPinLocalSessions;
  const onDeleteSession = remoteProject
    ? (sessionId: string) => {
        void remoteDelete([sessionId]);
      }
    : onDeleteLocalSession;
  const onDeleteSessions = remoteProject
    ? (sessionIds: readonly string[]) => {
        void remoteDelete(sessionIds);
      }
    : onDeleteLocalSessions;
  const onSetSessionLinkedWorkItem = remoteProject
    ? (sessionId: string, item: LinkedWorkItem | undefined) => {
        void remoteChange(sessionId, { linkedWorkItem: item ?? null });
      }
    : onSetLocalSessionLinkedWorkItem;
  const activeRemoteId = activeSessionId
    ? remoteSessionFor(activeSessionId)
    : undefined;
  const activeListedSessionId = remoteProject
    ? activeRemoteId
    : activeSessionId;
  const listedBusySessionIds = remoteProject
    ? new Set(
        remote.sessions
          .filter(
            (session) => session.status === "running" && !session.needsInput,
          )
          .map((session) => session.id),
      )
    : busySessionIds;
  const listedApprovalSessionIds = remoteProject
    ? new Set(
        remote.sessions
          .filter((session) => session.needsInput)
          .map((session) => session.id),
      )
    : approvalSessionIds;
  const projectSessions: SessionSummary[] = useMemo(
    () =>
      remoteProject
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
        : sessions,
    [remoteProject, remote.sessions, sessions, cwd],
  );
  const remoteExecutionCwd =
    remote.sessions.find((session) => session.id === activeRemoteId)?.cwd ??
    (activeSessionId ? remotePendingWorktree(activeSessionId) : undefined) ??
    (remoteProject && gitCwd && gitCwd !== cwd
      ? (parseRemotePath(gitCwd)?.hostPath ?? gitCwd)
      : undefined) ??
    undefined;
  const gitRoot =
    remoteProject && hostProject
      ? remotePath(
          hostProject.environmentId,
          remoteExecutionCwd ?? hostProject.cwd,
        )
      : gitCwd || cwd;
  const minWidth = inset ? INSET_MIN_WIDTH : MIN_WIDTH;
  const resize = useDragResize({
    min: minWidth,
    max: () => Math.min(MAX_WIDTH, Math.floor(window.innerWidth * 0.5)),
    defaultWidth: DEFAULT_WIDTH,
    initial: rememberedWidth,
    onCommit: (next) => {
      rememberedWidth = next;
    },
  });
  const { setPaneWidth: setSidebarWidth } = resize;
  const railSharedWidth = useMemo(
    () => ({
      width: resize.width,
      min: minWidth,
      max: () => Math.min(MAX_WIDTH, Math.floor(window.innerWidth * 0.5)),
      onCommit: (next: number) => {
        rememberedWidth = next;
        setSidebarWidth(next);
      },
    }),
    [resize.width, minWidth, setSidebarWidth],
  );
  const [tabOrder, setTabOrder] = useState<SidebarTab[]>(loadSidebarTabOrder);
  const [rightTabOrder, setRightTabOrder] =
    useState<RightPanelTab[]>(loadRightPanelTabOrder);
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
  const listableSessions = mergeFolderSessionSummaries(
    projectSessions,
    remoteProject ? [] : openSessions,
    sessionFolders,
  ).filter(
    (session) =>
      !isMonoSession(session.id) &&
      !("ephemeral" in session && session.ephemeral) &&
      !isHabitRun(session.id) &&
      !session.orchestrationLeadId &&
      inWorktreeFocus(session, focusedWorktree),
  );
  // Sessions saved hidden still show when a listed session started them:
  // /operator launches were once hidden like a Mono's by mistake. A Mono's
  // chat is never listed, so its hidden launches stay hidden.
  const shownSessionIds = new Set(
    listableSessions
      .filter((session) => !session.sidebarHidden)
      .map((session) => session.id),
  );
  const listedSessions = listableSessions.filter(
    (session) =>
      !session.sidebarHidden ||
      !!startedSessionCaller(session.id, shownSessionIds),
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
  // The inset list hangs the sessions an /operator thread started under it,
  // like an orchestrator's agents, rather than listing them on their own.
  const startedSessions = inset
    ? groupStartedSessions(visibleSessions)
    : NO_STARTED_SESSIONS;
  // Launches the sidebar has already shown for this project. Anything newer
  // grows in under its caller; what is there when a project opens does not.
  const knownStarted = useRef<{ cwd: string; ids: Set<string> } | null>(null);
  if (!knownStarted.current || knownStarted.current.cwd !== cwd)
    knownStarted.current = { cwd, ids: new Set(startedSessions.childIds) };
  const freshStarted = new Set(
    [...startedSessions.childIds].filter(
      (id) => !knownStarted.current!.ids.has(id),
    ),
  );
  useEffect(() => {
    for (const id of freshStarted) knownStarted.current?.ids.add(id);
  });
  const listedVisibleSessions = startedSessions.childIds.size
    ? visibleSessions.filter(
        (session) => !startedSessions.childIds.has(session.id),
      )
    : visibleSessions;
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
    listedVisibleSessions,
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
    listedVisibleSessions,
    sessionFolders,
    ungroupedVisible,
    pinnedSessionsCollapsed,
    reminderGroup,
  );
  const sessionListEntries = buildSessionList(
    listedVisibleSessions,
    sessionFolders,
    shownUngrouped,
    pinnedSessionsCollapsed,
    reminderGroup,
  );
  const firstLooseSessionIndex = sessionListEntries.findIndex(
    (entry) => entry.kind === "session",
  );
  // Reminders, folders and Pinned come first, under one label.
  const firstGroupIndex = sessionListEntries.findIndex(
    (entry) => entry.kind !== "session",
  );
  const groupFollows = (index: number) => {
    const next = sessionListEntries[index + 1];
    return !!next && next.kind !== "session";
  };
  // The inset list heads its loose sessions by when they were last active,
  // the order they are sorted in, so each label starts one unbroken run.
  const looseSessionLabels = new Map<number, string>();
  if (inset) {
    let previous: string | undefined;
    sessionListEntries.forEach((entry, index) => {
      if (entry.kind !== "session") return;
      const label = sessionAgeLabel(entry.session.updatedAt, now);
      if (label !== previous) looseSessionLabels.set(index, label);
      previous = label;
    });
  }
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
  const onReorderTabs = (ids: SidebarTab[]) => {
    const reordered = new Set(ids);
    let index = 0;
    const next = tabOrder.map((itemId) =>
      reordered.has(itemId) ? ids[index++] : itemId,
    );
    setTabOrder(next);
    saveSidebarTabOrder(next);
  };
  const onReorderRightTabs = (tabs: RightPanelTab[]) => {
    setRightTabOrder(tabs);
    saveRightPanelTabOrder(tabs);
  };
  const sortable = useAnimatedReorder(visibleTabs, onReorderTabs);
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
  // Likewise the session they cover stays current but isn't shown selected.
  const highlightedSessionId =
    otherViewActive || monoViewActive ? undefined : activeListedSessionId;
  // The inset sidebar holds the links to search, inbox, notes and
  // automations, and lets new users open their first project and Settings.
  // It stays available without a project; only settings take its place.
  // The classic sidebar only has project-scoped tools and sessions.
  const sidebarAvailable = inset
    ? !settingsOpen
    : !otherViewActive && !monoViewActive && inProject;
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
  // The inset layout peeks a collapsed sidebar out over the workspace while
  // the pointer rests on the window's left edge or the sidebar toggle.
  const peekMode = inset && !open && sidebarAvailable;
  const [peekOpen, setPeekOpen] = useState(false);
  const peekOpenRef = useRef(false);
  peekOpenRef.current = peekOpen;
  const peekRef = useRef<HTMLDivElement>(null);
  const peekBackdropRef = useRef<HTMLDivElement>(null);
  const peekVisible = peekMode && peekOpen;
  const [peekMounted, setPeekMounted] = useState(false);
  const peekClosing = peekMounted && !peekVisible && peekMode;
  const peekRendered = peekVisible || peekClosing;
  const peekAnimation = useRef<Animation[] | null>(null);
  const panelOpen = open || drawerVisible || peekVisible;
  // Keep the hidden explorer intact when a chat tab changes worktrees. Its
  // rows and file icons only need rebuilding when Files is actually shown.
  // The inset layout's project tools follow the right panel rather than the
  // sidebar's own tab.
  const paneHost = inset ? rightPanel : undefined;
  const paneTab: SidebarTab | RightPanelTab | null = paneHost
    ? paneHost.open && inProject
      ? paneHost.tab
      : null
    : tab;
  const panesOpen = paneHost ? paneTab != null : panelOpen;
  const explorer = useRef<{ cwd: string; rootLabel?: string } | null>(null);
  if (panesOpen && paneTab === "files") {
    explorer.current = { cwd: gitRoot, rootLabel: explorerRootLabel };
  }
  const gitStatuses = useGitFileStatuses(
    gitRoot,
    panesOpen && paneTab === "files",
  );
  const changeStats = useProjectDiffStats(gitRoot, panesOpen);

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
    if (!peekMode) setPeekOpen(false);
  }, [peekMode]);

  useEffect(() => {
    if (peekVisible) setPeekMounted(true);
    else if (!peekClosing) setPeekMounted(false);
  }, [peekVisible, peekClosing]);

  // Fade the panel in with a short drift from the edge, and the dim behind
  // it. A reversal midway starts from wherever both currently are.
  useLayoutEffect(() => {
    const panel = peekRef.current;
    const backdrop = peekBackdropRef.current;
    if (!peekRendered || !panel || !backdrop) {
      peekAnimation.current = null;
      return;
    }
    const hidden = { opacity: "0", transform: "translateX(-12px)" };
    const shown = { opacity: "1", transform: "none" };
    const running = peekAnimation.current;
    const from = running
      ? {
          opacity: getComputedStyle(panel).opacity,
          transform: getComputedStyle(panel).transform,
        }
      : peekClosing
        ? shown
        : hidden;
    const fromDim = running
      ? getComputedStyle(backdrop).opacity
      : peekClosing
        ? "1"
        : "0";
    running?.forEach((animation) => animation.cancel());
    peekAnimation.current = null;
    const reduceMotion = window.matchMedia?.(
      "(prefers-reduced-motion: reduce)",
    ).matches;
    if (typeof panel.animate !== "function" || reduceMotion) {
      if (peekClosing) setPeekMounted(false);
      return;
    }
    const timing: KeyframeAnimationOptions = peekClosing
      ? {
          duration: 200,
          easing: "cubic-bezier(0.4, 0, 0.2, 1)",
          fill: "forwards",
        }
      : { duration: 240, easing: "cubic-bezier(0.16, 1, 0.3, 1)" };
    const fade = panel.animate([from, peekClosing ? hidden : shown], timing);
    const dim = backdrop.animate(
      [{ opacity: fromDim }, { opacity: peekClosing ? "0" : "1" }],
      timing,
    );
    const animations = [fade, dim];
    peekAnimation.current = animations;
    fade.onfinish = () => {
      if (peekAnimation.current !== animations) return;
      peekAnimation.current = null;
      if (peekClosing) setPeekMounted(false);
    };
  }, [peekRendered, peekClosing]);

  useEffect(() => {
    if (!peekMode) return;
    let closeTimer: number | undefined;
    const cancelClose = () => {
      window.clearTimeout(closeTimer);
      closeTimer = undefined;
    };
    const scheduleClose = () => {
      if (closeTimer !== undefined) return;
      closeTimer = window.setTimeout(() => {
        closeTimer = undefined;
        setPeekOpen(false);
      }, PEEK_CLOSE_DELAY);
    };
    const onPointerMove = (event: PointerEvent) => {
      const el = event.target instanceof Element ? event.target : null;
      const onTrigger =
        !!el?.closest("[data-sidebar-peek-trigger]") ||
        (event.clientX <= PEEK_EDGE && !el?.closest("[data-inset-title-bar]"));
      if (onTrigger) {
        cancelClose();
        setPeekOpen(true);
        return;
      }
      if (!peekOpenRef.current) return;
      // Menus opened from the panel keep it out, as does a drag under way.
      if (
        peekRef.current?.contains(el) ||
        el?.closest("[data-popover-side]") ||
        document.querySelector(PEEK_HOLDING_MENU) ||
        event.buttons !== 0
      ) {
        cancelClose();
        return;
      }
      scheduleClose();
    };
    const onPointerLeave = () => {
      if (peekOpenRef.current && !document.querySelector(PEEK_HOLDING_MENU))
        scheduleClose();
    };
    // A click anywhere else puts the sidebar away at once.
    const onPointerDown = (event: PointerEvent) => {
      const el = event.target instanceof Element ? event.target : null;
      if (
        peekRef.current?.contains(el) ||
        el?.closest("[data-sidebar-peek-trigger],[data-popover-side]")
      ) {
        return;
      }
      cancelClose();
      setPeekOpen(false);
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.defaultPrevented) return;
      if (!peekOpenRef.current) return;
      cancelClose();
      setPeekOpen(false);
    };
    window.addEventListener("pointermove", onPointerMove);
    document.documentElement.addEventListener("pointerleave", onPointerLeave);
    window.addEventListener("pointerdown", onPointerDown);
    window.addEventListener("keydown", onKeyDown);
    return () => {
      cancelClose();
      window.removeEventListener("pointermove", onPointerMove);
      document.documentElement.removeEventListener(
        "pointerleave",
        onPointerLeave,
      );
      window.removeEventListener("pointerdown", onPointerDown);
      window.removeEventListener("keydown", onKeyDown);
    };
  }, [peekMode]);

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
    for (const session of remoteProject ? [] : openSessions)
      known.add(session.id);
    if (activeListedSessionId) known.add(activeListedSessionId);
    if (remoteProject && activeSessionId) known.add(activeSessionId);
    if (remoteProject) {
      for (const folder of sessionFolders) {
        for (const shellId of folder.sessionIds) {
          const hostId = remoteSessionFor(shellId);
          if (hostId && known.has(hostId))
            completedFolderSessions.set(shellId, hostId);
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
            sessionIds: folder.sessionIds.map(
              (id) => completedFolderSessions.get(id) ?? id,
            ),
          }))
        : current;
      const next = pruneSessionFolders(migrated, known);
      if (next === current) return current;
      saveSessionFolders(cwd, next);
      return next;
    });
  }, [
    activeListedSessionId,
    activeSessionId,
    cwd,
    openSessions,
    pending,
    projectSessions,
    remoteProject,
    remote.loaded,
    sessionFolders,
    status,
  ]);

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
    { kind: "item", id: "rename", label: t("Rename"), shortcut: "F2" },
    { kind: "sep" },
    { kind: "item", id: "ungroup", label: t("Ungroup") },
  ];
  const sessionMenuItems: ExplorerMenuItem[] = [
    ...(onCancelReminders && menuReminderTimes.length > 0
      ? [
          {
            kind: "item" as const,
            id: "reminder:cancel",
            label: t("Cancel reminder"),
            description:
              menuReminderTimes.length === 1
                ? formatReminderTime(menuReminderTimes[0])
                : t("Multiple reminder times"),
          },
          { kind: "sep" as const },
        ]
      : []),
    ...(onPinSession || onPinSessions
      ? [
          {
            kind: "item" as const,
            id: "pin",
            label: allMenuSessionsPinned ? t("Unpin") : t("Pin"),
          },
        ]
      : []),
    ...(!multipleMenuSessions && onRenameSession
      ? [
          {
            kind: "item" as const,
            id: "rename",
            label: t("Rename"),
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
    {
      kind: "item",
      id: "reminder",
      label: t("Remind me"),
      disabled: !onSetReminders,
      submenu: sessionReminderPresets(),
    },
    { kind: "sep" as const },
    { kind: "item" as const, id: "folder-new", label: t("New folder") },
    ...(sessionFolders.length > 0 ? [{ kind: "sep" as const }] : []),
    ...sessionFolders.map((folder) => ({
      kind: "item" as const,
      id: `folder-add:${folder.id}`,
      label: t("Add to {name}", { name: folder.name }),
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
              ? t("Remove from folders")
              : t("Remove from folder"),
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
                  label: allMenuSessionsArchived ? t("Unarchive") : t("Archive"),
                },
              ]
            : []),
          ...(onDeleteSession || onDeleteSessions
            ? [
                {
                  kind: "item" as const,
                  id: "delete",
                  label: t("Delete"),
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
      const anchor =
        selectionAnchorRef.current ?? activeListedSessionId ?? sessionId;
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

  // A session row, with the sessions it started hanging off a rail below it.
  const renderSessionCard = (session: SessionSummary, compact = false) => {
    const started = startedSessions.byParent.get(session.id);
    if (!started?.length) return renderSessionRow(session, compact);
    // Open while the thread or one of its sessions is the one in view, chosen
    // or working, as an orchestrator's agents are.
    const inPlay = (id: string) =>
      id === highlightedSessionId ||
      selectedSessionIds.has(id) ||
      listedBusySessionIds.has(id);
    const open =
      inPlay(session.id) || started.some((child) => inPlay(child.id));
    const card = renderSessionRow(session, compact, {
      startedCount: started.length,
      startedOpen: open,
      startedInView: started.some(
        (child) =>
          child.id === highlightedSessionId || selectedSessionIds.has(child.id),
      ),
    });
    return (
      <>
        {card}
        {/* The rail drops from under the row's mark, as an orchestrator's
            agents do. New launches grow in under it; the whole rail does
            when it is the first. */}
        <RevealHeight
          className="ml-2.5"
          data-started-sessions={session.id}
          open={open}
          animateIn={started.every((child) => freshStarted.has(child.id))}
        >
          <div className="pt-1">
            {started.map((child) => (
              <RevealHeight
                key={child.id}
                className="zen-phase-step orchestration-tree-step"
                animateIn={
                  freshStarted.has(child.id) &&
                  !started.every((other) => freshStarted.has(other.id))
                }
              >
                {renderSessionRow(child, compact, { nested: true })}
              </RevealHeight>
            ))}
          </div>
        </RevealHeight>
      </>
    );
  };

  const renderSessionRow = (
    session: SessionSummary,
    compact = false,
    {
      nested = false,
      startedCount = 0,
      startedOpen = false,
      startedInView = false,
    }: {
      nested?: boolean;
      startedCount?: number;
      startedOpen?: boolean;
      startedInView?: boolean;
    } = {},
  ) =>
    renamingSessionId === session.id && onRenameSession ? (
      <SessionRenameRow
        session={session}
        isActive={session.id === highlightedSessionId}
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
        isActive={session.id === highlightedSessionId}
        isSelected={selectedSessionIds.has(session.id)}
        busy={listedBusySessionIds.has(session.id)}
        done={unseenFinishedIds.has(session.id)}
        linkedUpdate={linkedSessionUpdateIds.has(session.id)}
        needsApproval={listedApprovalSessionIds.has(session.id)}
        dropTarget={isSessionDrop("session", session.id)}
        compact={compact}
        dense={inset}
        nested={nested}
        startedCount={startedCount}
        startedOpen={startedOpen}
        startedInView={startedInView}
        now={now}
        onSelect={cardActions.select}
        onOpenWorkItem={
          onOpenInboxItem && !remoteProject
            ? cardActions.openWorkItem
            : undefined
        }
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
      placeholder={t("Search conversations...")}
      aria-label={t("Search conversations")}
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
  const hasUnseenFinished = listedSessions.some((session) =>
    unseenFinishedIds.has(session.id),
  );
  const hasChangeStats = changeAdditions > 0 || changeDeletions > 0;
  const changesLabel = hasChangeStats
    ? [
        t("Changes"),
        changeAdditions > 0 ? `+${changeAdditions}` : "",
        changeDeletions > 0 ? `-${changeDeletions}` : "",
      ]
        .filter(Boolean)
        .join(" ")
    : t("Changes");

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
              {t(TAB_LABELS[itemId])}
            </span>
          )}
        </button>
      </div>
    );
  });

  const projectLabel = useProjectLabel(cwd);
  const [insetInboxMenu, setInsetInboxMenu] = useState<{
    x: number;
    y: number;
  } | null>(null);
  const insetInboxTrigger = useRef<HTMLElement | null>(null);
  useEffect(() => {
    if (!inset || !panelOpen || !sidebarAvailable) setInsetInboxMenu(null);
  }, [inset, panelOpen, sidebarAvailable]);
  const headerProjectMenu = useProjectMenu({
    onRemoveProject,
    onOpenNotificationSettings,
    onOpen: () => setInsetInboxMenu(null),
    // Only the inset layout uses this menu, and it has no project rail.
    railOptions: false,
  });
  // The inset layout names the project here, with its settings and the
  // project and worktree pickers in a menu below it.
  const headerProject = inset
    ? {
        name: projectLabel,
        icon: (
          <CurrentProjectIcon
            cwd={cwd}
            busy={projectPathBusy(busyProjectPaths, cwd)}
            mascotClassName="size-2.5"
          />
        ),
        onOpenSettings,
        settingsShortcut: `${MOD},`,
        keepOpen: headerProjectMenu.isActive,
        // The right-click project menu, as a flyout level with its row.
        onOpenProjectSettings: (row: HTMLElement) => {
          const rect = row.getBoundingClientRect();
          headerProjectMenu.open(cwd, rect.right, rect.top - 8, row);
        },
        onContextMenu: (x: number, y: number, trigger: HTMLElement) =>
          headerProjectMenu.open(cwd, x, y, trigger),
        onCloseProjectSettings: headerProjectMenu.close,
        projectSettingsOpen: headerProjectMenu.isOpen,
        renderProjectPicker: onSelectProject
          ? ({ anchor, layer, attributes, onClose }: ProjectMenuFlyout) => (
              <ProjectPickerPopover
                anchor={anchor}
                flyout={{ layer, attributes }}
                projects={pickerProjects(recents, cwd)}
                currentProject={cwd}
                onDismiss={onClose}
                onSelectProject={onSelectProject}
                onOpenProject={onOpenProject}
                onProjectContextMenu={headerProjectMenu.open}
                projectMenuActive={headerProjectMenu.isActive}
              />
            )
          : undefined,
      }
    : undefined;
  // The inset sidebar's spaces: the projects in the rail's order, the same
  // one Project: Previous / Next steps through.
  const spaceProjects = useMemo(
    () =>
      inset ? projectRailItems(recents, cwd).map((item) => item.path) : [],
    [inset, recents, cwd],
  );
  const swipeDestination = useRef<{ from: string; to: string } | null>(null);
  const onSwipeProject = useCallback(
    (path: string) => {
      swipeDestination.current = { from: cwd, to: path };
      onSelectProject?.(path);
    },
    [cwd, onSelectProject],
  );
  const projectSwipeRef = useProjectSwipe(inset, {
    cwd,
    projects: spaceProjects,
    onSelectProject: onSelectProject ? onSwipeProject : undefined,
  });
  const setSidebarRef = useCallback(
    (el: HTMLElement | null) => {
      resize.setPaneRef(el);
      projectSwipeRef(el);
    },
    [resize.setPaneRef, projectSwipeRef],
  );
  // Projects the rail never placed fall back to most-recent-first, which
  // reshuffles the spaces on every switch. The rail is not mounted in this
  // layout to save its order, so the spaces pin down whatever they show.
  useEffect(() => {
    if (!inset || !spaceProjects.length) return;
    const stored = loadProjectRailOrder();
    const unplaced = spaceProjects.some(
      (path) => !stored.some((saved) => sameProjectPath(saved, path)),
    );
    if (unplaced) saveProjectRailOrder(spaceProjects);
  }, [inset, spaceProjects]);
  const slideRefs = useRef<(HTMLElement | null)[]>([]);
  const previousSpace = useRef(cwd);
  // The outgoing project's header and list, copied while they are still on
  // screen, so they can be pushed away as the new ones arrive.
  const outgoingSpace = useRef<{
    to: string;
    nodes: { node: HTMLElement; scrollTop: number }[];
  } | null>(null);
  if (
    inset &&
    !sameProjectPath(previousSpace.current, cwd) &&
    outgoingSpace.current?.to !== cwd
  ) {
    outgoingSpace.current = {
      to: cwd,
      nodes: slideRefs.current.flatMap((el) => {
        if (!el?.isConnected) return [];
        const node = el.cloneNode(true) as HTMLElement;
        // A slide still holding the live copy hidden must not hide this one.
        node.style.opacity = "";
        return [{ node, scrollTop: el.scrollTop }];
      }),
    };
  }
  const spaceProjectsRef = useRef(spaceProjects);
  spaceProjectsRef.current = spaceProjects;
  const spaceSlide = useRef<{
    direction: number;
    swipe: boolean;
    overlay: HTMLDivElement | null;
    started: boolean;
    timer?: number;
    animations: Animation[];
  } | null>(null);
  const endSpaceSlide = useCallback(() => {
    const slide = spaceSlide.current;
    if (!slide) return;
    spaceSlide.current = null;
    window.clearTimeout(slide.timer);
    for (const animation of slide.animations) animation.cancel();
    slide.overlay?.remove();
    for (const el of slideRefs.current) if (el) el.style.opacity = "";
  }, []);
  // The push itself: both projects move the sidebar's full width together,
  // the old one fading out as the new one fades in.
  const startSpaceSlide = useCallback(() => {
    const slide = spaceSlide.current;
    if (!slide || slide.started) return;
    slide.started = true;
    window.clearTimeout(slide.timer);
    const aside = slideRefs.current[0]?.closest("aside");
    const distance = aside?.clientWidth ?? 240;
    const timing = slide.swipe
      ? { duration: 180, easing: "cubic-bezier(0.16, 1, 0.3, 1)" }
      : { duration: 440, easing: "cubic-bezier(0.4, 0, 0.2, 1)" };
    for (const el of slideRefs.current) {
      if (!el || typeof el.animate !== "function") continue;
      el.style.opacity = "";
      slide.animations.push(
        el.animate(
          [
            {
              transform: `translateX(${slide.direction * distance}px)`,
              opacity: 0,
            },
            { transform: "none", opacity: 1 },
          ],
          timing,
        ),
      );
    }
    for (const node of slide.overlay?.children ?? []) {
      slide.animations.push(
        (node as HTMLElement).animate(
          [
            { transform: "none", opacity: 1 },
            {
              transform: `translateX(${-slide.direction * distance}px)`,
              opacity: 0,
            },
          ],
          { ...timing, fill: "forwards" },
        ),
      );
    }
    void Promise.all(slide.animations.map((animation) => animation.finished))
      .then(() => {
        if (spaceSlide.current === slide) endSpaceSlide();
      })
      .catch(() => undefined);
  }, [endSpaceSlide]);
  // Switching projects slides the sidebar the way the spaces run, like Zen's
  // workspaces: moving right, the old project leaves to the left as the new
  // one comes in from the right, and the reverse moving left.
  useLayoutEffect(() => {
    const from = previousSpace.current;
    previousSpace.current = cwd;
    const outgoing = outgoingSpace.current;
    outgoingSpace.current = null;
    if (sameProjectPath(from, cwd)) return;
    const destination = swipeDestination.current;
    swipeDestination.current = null;
    const swipe =
      !!destination &&
      sameProjectPath(destination.from, from) &&
      sameProjectPath(destination.to, cwd);
    const step = projectStep;
    projectStep = null;
    if (!inset) return;
    endSpaceSlide();
    const spaces = spaceProjectsRef.current;
    const fromIndex = spaces.findIndex((path) => sameProjectPath(path, from));
    const toIndex = spaces.findIndex((path) => sameProjectPath(path, cwd));
    if (fromIndex < 0 || toIndex < 0) return;
    if (window.matchMedia?.("(prefers-reduced-motion: reduce)").matches) return;
    if (slideRefs.current.some((el) => el && typeof el.animate !== "function"))
      return;
    // The copies sit on an overlay over the sidebar, each where its live
    // counterpart now is, covering it until the push starts.
    const aside = slideRefs.current[0]?.closest("aside");
    let overlay: HTMLDivElement | null = null;
    if (aside && outgoing?.nodes.length) {
      overlay = document.createElement("div");
      overlay.setAttribute("aria-hidden", "true");
      overlay.inert = true;
      overlay.className =
        "pointer-events-none absolute inset-0 z-20 overflow-hidden";
      const asideRect = aside.getBoundingClientRect();
      outgoing.nodes.forEach(({ node, scrollTop }, index) => {
        const live = slideRefs.current[index];
        if (!live) return;
        const rect = live.getBoundingClientRect();
        Object.assign(node.style, {
          position: "absolute",
          left: `${rect.left - asideRect.left}px`,
          top: `${rect.top - asideRect.top}px`,
          width: `${rect.width}px`,
          height: `${rect.height}px`,
        });
        overlay!.append(node);
        node.scrollTop = scrollTop;
      });
      aside.append(overlay);
    }
    // Clicks and shortcuts can wait for first-load sessions. A swipe starts
    // immediately so disk/network latency does not hold the gesture still.
    for (const el of slideRefs.current) if (el) el.style.opacity = "0";
    spaceSlide.current = {
      // A step past either end wraps around but keeps moving the same way.
      direction:
        step && sameProjectPath(step.to, cwd)
          ? step.direction
          : toIndex > fromIndex
            ? 1
            : -1,
      overlay,
      swipe,
      started: false,
      animations: [],
      timer: swipe ? undefined : window.setTimeout(startSpaceSlide, 400),
    };
    if (swipe) startSpaceSlide();
  }, [cwd, inset, endSpaceSlide, startSpaceSlide]);
  useLayoutEffect(() => {
    if (spaceSlide.current && !pendingFirstLoad) startSpaceSlide();
  }, [cwd, pendingFirstLoad, startSpaceSlide]);
  useEffect(() => endSpaceSlide, [endSpaceSlide]);

  const workspaceHeader = (
    <div
      ref={(el) => {
        slideRefs.current[0] = el;
      }}
      // Inset, it sits just under the title bar, so it keeps less above the
      // project button than below it, and its search icon lines up with the
      // sessions' filter and times.
      className={`flex shrink-0 select-none items-center gap-1 pl-3${
        inset ? " h-[39px] pb-px pr-3" : " h-10 border-b border-stroke pr-1.5"
      }`}
      data-tauri-drag-region="deep"
    >
      <div className="flex min-w-0 flex-1 items-center">
        {(headerProject || !remoteProject) && cwd && cwd !== "~" ? (
          <SidebarWorktreeSwitcher
            worktrees={!remoteProject}
            cwd={cwd}
            tabStats={worktreeTabStats}
            onSelect={onSelectWorkspace}
            pending={workspaceSwitchPending}
            switchError={workspaceSwitchError}
            project={headerProject}
          />
        ) : (
          <span className="min-w-0 truncate text-sm font-medium leading-tight">
            {t("Workspace")}
          </span>
        )}
      </div>
      {/* Inset, a new session is the + at the end of the title bar's tabs. */}
      <WorkspaceTitleActions
        onSearch={onGoToFile}
        onNew={inset ? undefined : onNew}
      />
      {headerProjectMenu.element}
    </div>
  );

  // The inset list names its loose sessions like the groups above them, and
  // carries the filter the search row holds elsewhere.
  const sessionsFilterButton = (
    <SessionsHeaderButton
      label="Filter sessions"
      active={filtersActive}
      open={!!filterMenu}
      hasPopup
      onClick={onFilterButtonClick}
    >
      <ListFilter className="size-3" strokeWidth={1.75} />
    </SessionsHeaderButton>
  );
  // Stands in for the date labels when no loose session is listed, so the
  // filter stays reachable.
  const sessionsLabel = inset ? (
    <SidebarSectionLabel label="Sessions">
      {sessionsFilterButton}
    </SidebarSectionLabel>
  ) : null;

  const navList = inset ? (
    <SidebarNavList
      search={
        onSearch ? { active: searchActive, onClick: onSearch } : undefined
      }
      items={[
        onOpenInbox && {
          label: "Inbox",
          icon: Inbox,
          active: inboxActive,
          dot: inboxUnseen,
          onClick: onOpenInbox,
          onOpenContextMenu: (x, y) => {
            headerProjectMenu.close();
            insetInboxTrigger.current =
              document.activeElement instanceof HTMLElement
                ? document.activeElement
                : null;
            setInsetInboxMenu({ x, y });
          },
        },
        notesEnabled &&
          onOpenNotes && {
            label: "Notes",
            icon: StickyNote,
            active: notesActive,
            onClick: onOpenNotes,
          },
        onOpenAutomations && {
          label: "Automations",
          icon: Zap,
          active: automationsActive,
          onClick: onOpenAutomations,
        },
      ]}
    />
  ) : null;

  const filesPane = (
    <div
      className={`flex min-h-0 flex-1 flex-col overflow-hidden ${
        paneTab === "files" ? "" : "hidden"
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
              compactHeader={inset}
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
  );
  const changesPane = (
    <>
      {paneTab === "changes" ? (
        <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
          <SourceControl
            cwd={gitRoot}
            enabled={panesOpen}
            showGraph={!inset}
            compactHeader={inset}
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
    </>
  );

  const graphPane = paneHost && paneTab === "graph" ? (
    <div
      role="tabpanel"
      aria-label="History"
      className="flex min-h-0 flex-1 flex-col overflow-hidden"
    >
      <GitHistoryGraph
        cwd={gitRoot}
        enabled={panesOpen}
        selectedSha={selectedCommitSha}
        onOpenCommit={onOpenCommit ?? (() => {})}
      />
    </div>
  ) : null;

  const sidebarContent = (
    <aside
      ref={setSidebarRef}
      // The inset card's own edge separates it from the sidebar.
      className={`relative flex h-full min-h-0 shrink-0 flex-col${
        inset
          ? " inset-sidebar overflow-x-clip"
          : " body-glass border-r border-stroke"
      }`}
    >
      {/* The inset layout keeps the open-rail header either way: its menu
          switches projects. */}
      {railVisible || inset ? (
        <>
          {workspaceHeader}
          {/* The inset sidebar has no tabs: the project tools live in the
              right panel. */}
          {inset ? null : (
            <div
              role="tablist"
              aria-label={t("Workspace")}
              className="flex h-9 shrink-0 items-center gap-px border-b border-stroke px-2"
            >
              {workspaceTabItems}
            </div>
          )}
        </>
      ) : (
        <>
          {titleBarAbove ? null : (
            <div
              className="flex h-10 shrink-0 select-none items-center border-b border-stroke pr-1.5"
              data-tauri-drag-region="deep"
            >
              {trafficLightsVisible ? <div className="w-[78px] shrink-0" /> : null}
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
        {paneHost ? null : filesPane}
        {tab === "sessions" && cwd && cwd !== "~" && !inset ? (
          <div className="flex h-9 shrink-0 items-center gap-1 border-b border-stroke px-2">
            <div className="relative flex h-7 min-w-0 flex-1 items-center">
              <Search className="pointer-events-none absolute left-2 size-3 shrink-0 opacity-50" />
              {sessionSearchInput}
            </div>
            <SessionsHeaderButton
              label={t("Filter sessions")}
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
            slideRefs.current[1] = el;
          }}
          className={`sidebar-session-scroll min-h-0 flex-1 overflow-y-auto overscroll-none ${
            tab === "sessions" ? "" : "hidden"
          }`}
          // Marks a scrolled list straight on the element, so the header's
          // fade comes and goes without re-rendering the rows.
          onScroll={
            inset
              ? (event) =>
                  event.currentTarget.toggleAttribute(
                    "data-scrolled",
                    event.currentTarget.scrollTop > 0,
                  )
              : undefined
          }
        >
          {navList}
          {/* The project's Monos, above its folders. */}
          {inset && railMonos && inProject ? (
            <MonoRailSection {...railMonos} project={cwd} />
          ) : null}
          {!cwd || cwd === "~" ? (
            <div className="px-3 py-2 text-[12px] text-content/50">
              <p>{t("No project folder")}</p>
              {inset && onOpenProject ? (
                <button
                  type="button"
                  onClick={onOpenProject}
                  className="mt-2 flex items-center gap-1.5 rounded-md bg-content/5 px-2 py-1.5 text-content/70 hover:bg-content/10 hover:text-content"
                >
                  <Plus className="size-3.5" strokeWidth={1.75} />
                  Open project
                </button>
              ) : null}
            </div>
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
                  {t("Couldn’t load sessions")}
                </p>
              ) : visibleSessions.length === 0 ? (
                // Inset like the list it stands in for, so the label and
                // its text line up with the sections above.
                <div className={inset ? "px-2 pt-1" : undefined}>
                  {sessionsLabel}
                  {/* A narrowed-down result is a transient answer to what the user
                    just typed, so it stays a quiet line of text. Only the genuine
                    "this project has nothing in it" case earns the illustration. */}
                  {narrowedByUser ? (
                    <p
                      className={`${inset ? "px-2" : "px-3"} py-2 text-[12px] text-content/50`}
                    >
                      {searchNarrowed
                        ? t("No matching sessions")
                        : t("No sessions match these filters")}
                    </p>
                  ) : remoteProject && !remote.machine ? (
                    <p
                      className={`${inset ? "px-2" : "px-3"} py-2 text-[12px] text-content/45`}
                    >
                      This project’s machine isn’t connected on this computer.
                    </p>
                  ) : (
                    <SessionsEmpty
                      message={t("Sessions you start will show up here")}
                    />
                  )}
                </div>
              ) : (
                <ul
                  data-session-list
                  className={`flex flex-col pb-10 ${
                    inset ? "gap-px px-2 pt-1" : "gap-0.5 p-1.5"
                  }`}
                >
                  {sessionListEntries.map((entry, index) => {
                    if (entry.kind === "pinned" || entry.kind === "reminders") {
                      const isReminders = entry.kind === "reminders";
                      const expanded = searchNarrowed || !entry.collapsed;
                      const beforeUngrouped =
                        sessionListEntries[index + 1]?.kind === "session";
                      const groupItem = (
                        <li
                          key={`${entry.kind}-sessions`}
                          data-pinned-sessions={isReminders ? undefined : ""}
                          data-reminder-sessions={isReminders ? "" : undefined}
                          className={`relative ${
                            inset
                              ? groupFollows(index)
                                ? "mb-0.5"
                                : "mb-3"
                              : expanded || beforeUngrouped
                                ? "mb-1.5"
                                : ""
                          }`}
                        >
                          <div
                            className={
                              inset
                                ? ""
                                : "overflow-hidden rounded-md bg-content/5"
                            }
                          >
                            <FolderRow
                              folder={
                                isReminders
                                  ? {
                                      name: t("Reminders"),
                                      customColor: REMINDERS_COLOR,
                                    }
                                  : { name: t("Pinned") }
                              }
                              sessions={entry.sessions}
                              expanded={expanded}
                              dense={inset}
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
                                    className={inset ? "size-4" : "size-3.5"}
                                    strokeWidth={1.75}
                                  />
                                ) : (
                                  <Pin
                                    className={
                                      inset ? "size-4" : "size-3.5 text-content"
                                    }
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
                              <ul
                                className={`flex flex-col gap-px ${
                                  inset ? "pt-px" : "p-1"
                                }`}
                              >
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
                      return inset && index === firstGroupIndex ? (
                        <Fragment key={`${entry.kind}-sessions`}>
                          <li className="list-none">
                            <SidebarSectionLabel label="Folders" />
                          </li>
                          {groupItem}
                        </Fragment>
                      ) : (
                        groupItem
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
                      const folderItem = (
                        <li
                          key={entry.folder.id}
                          ref={(el) =>
                            folderSortable.setItemRef(entry.folder.id, el)
                          }
                          data-session-folder={entry.folder.id}
                          className={`relative ${
                            inset
                              ? groupFollows(index)
                                ? "mb-0.5"
                                : "mb-3"
                              : expanded || beforeUngrouped
                                ? "mb-1.5"
                                : ""
                          } ${draggingFolder ? "opacity-40" : ""}`}
                        >
                          {showFolderDropStart ? (
                            <div className="pointer-events-none absolute inset-x-1 top-0 z-20 h-0.5 rounded-full bg-accent" />
                          ) : null}
                          {showFolderDropEnd ? (
                            <div className="pointer-events-none absolute inset-x-1 bottom-0 z-20 h-0.5 rounded-full bg-accent" />
                          ) : null}
                          <div
                            className={
                              inset
                                ? ""
                                : `overflow-hidden rounded-md ${
                                    shellFill ? "" : "bg-content/5"
                                  }`
                            }
                            style={
                              shellFill && !inset
                                ? { background: shellFill }
                                : undefined
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
                                dense={inset}
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
                                <ul
                                  className={`flex flex-col gap-px ${
                                    inset ? "pt-px" : "p-1"
                                  }`}
                                >
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
                                  <div
                                    className={
                                      inset
                                        ? "pt-px"
                                        : "border-t border-stroke p-1"
                                    }
                                  >
                                    <button
                                      type="button"
                                      data-no-drag
                                      data-tauri-drag-region="false"
                                      title={t("New session")}
                                      aria-label={t("New session")}
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
                                        {t("New session")}
                                      </span>
                                    </button>
                                  </div>
                                ) : null}
                              </>
                            ) : null}
                          </div>
                        </li>
                      );
                      // The inset list groups its folders under one label.
                      return inset && index === firstGroupIndex ? (
                        <Fragment key={entry.folder.id}>
                          <li className="list-none">
                            <SidebarSectionLabel label="Folders" />
                          </li>
                          {folderItem}
                        </Fragment>
                      ) : (
                        folderItem
                      );
                    }
                    const item = (
                      <SessionListItem
                        key={entry.session.id}
                        session={entry.session}
                        cwd={cwd}
                        motion={sessionInsertMotion}
                      >
                        {renderSessionCard(entry.session)}
                      </SessionListItem>
                    );
                    const ageLabel = looseSessionLabels.get(index);
                    return ageLabel ? (
                      <Fragment key={entry.session.id}>
                        <li
                          className={`list-none ${
                            index === firstLooseSessionIndex ? "" : "mt-3"
                          }`}
                        >
                          <SidebarSectionLabel label={ageLabel}>
                            {index === firstLooseSessionIndex
                              ? sessionsFilterButton
                              : null}
                          </SidebarSectionLabel>
                        </li>
                        {item}
                      </Fragment>
                    ) : (
                      item
                    );
                  })}
                  {sessionsLabel && firstLooseSessionIndex < 0 ? (
                    <li className="list-none">{sessionsLabel}</li>
                  ) : null}
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
        {paneHost ? null : changesPane}
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
              {/* Without a project, the inset header has no Settings menu. */}
              {(inset ? !inProject : !compactProjectRail) ? (
                <RailAction
                  label={t("Settings")}
                  icon={Settings}
                  onClick={onOpenSettings}
                  shortcut={`${MOD},`}
                  ariaLabel={`${t("Settings")} (${MOD},)`}
                />
              ) : null}
            </div>
          </>
        ) : null}
        {inset && onSelectProject ? (
          <ProjectSpacesBar
            projects={spaceProjects}
            cwd={cwd}
            busyProjectPaths={busyProjectPaths}
            onSelectProject={onSelectProject}
            onProjectContextMenu={headerProjectMenu.open}
          />
        ) : null}
      </>
      {insetInboxMenu ? (
        <InboxNotificationMenu
          {...insetInboxMenu}
          projectPaths={[...collectRailProjects(recents, cwd).keys()]}
          onOpenSettings={onOpenNotificationSettings}
          onClose={() => {
            setInsetInboxMenu(null);
            insetInboxTrigger.current?.focus();
          }}
        />
      ) : null}
      {sessionMenu ? (
        <ExplorerMenu
          x={sessionMenu.x}
          y={sessionMenu.y}
          items={sessionMenuItems}
          ariaLabel={
            multipleMenuSessions
              ? t("{count} selected session actions", {
                  count: menuSessionIds.length,
                })
              : t("Session actions")
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
          ariaLabel={t("Folder actions")}
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
        aria-valuemin={minWidth}
        aria-valuemax={MAX_WIDTH}
        className={`absolute inset-y-0 -right-px z-10 w-1.5 cursor-col-resize touch-none${
          inset
            ? ""
            : resize.dragging
              ? " bg-content/15"
              : " hover:bg-content/10"
        }`}
        onPointerDown={resize.onPointerDown}
        onDoubleClick={resize.onDoubleClick}
      />
    </aside>
  );

  const rails = (
    <>
      {compactRailVisible ? (
        <CompactProjectRail
          cwd={cwd}
          recents={recents}
          busy={projectPathBusy(busyProjectPaths, cwd)}
          tabs={visibleTabs}
          activeTab={tab}
          tabShown={panelOpen}
          changesLabel={changesLabel}
          changeAdditions={changeAdditions}
          changeDeletions={changeDeletions}
          hasChanges={hasChanges}
          hasUnseenFinished={hasUnseenFinished}
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
          headerless={inset}
          // In Settings the inset sidebar's width carries over.
          sharedWidth={inset ? railSharedWidth : undefined}
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
    </>
  );

  return (
    <div
      className={`flex h-full shrink-0 ${
        railVisible || compactRailVisible || sidebarVisible || drawerRendered
          ? ""
          : "hidden"
      }`}
    >
      {rails}
      {sidebarVisible ? sidebarContent : null}
      {paneHost?.host && paneTab
        ? createPortal(
            <RightPanel
              tab={paneHost.tab}
              tabs={rightTabOrder}
              onReorder={onReorderRightTabs}
              changesLabel={changesLabel}
              hasChangeStats={hasChangeStats}
              changeAdditions={changeAdditions}
              changeDeletions={changeDeletions}
              onTabChange={paneHost.onTabChange}
            >
              {filesPane}
              {changesPane}
              {graphPane}
            </RightPanel>,
            paneHost.host,
          )
        : null}
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
      {peekRendered
        ? createPortal(
            <>
              <div
                ref={peekBackdropRef}
                aria-hidden
                className="sidebar-peek-backdrop pointer-events-none fixed inset-0"
                style={{ zIndex: LAYER.peek }}
              />
              <div
                ref={peekRef}
                data-sidebar-peek={peekClosing ? "closing" : "open"}
                inert={peekClosing || undefined}
                className={`sidebar-peek fixed bottom-1.5 left-1.5 top-10 flex overflow-hidden rounded-[10px] border text-content ${
                  peekClosing ? "pointer-events-none" : ""
                }`}
                style={{ zIndex: LAYER.peek }}
              >
                {sidebarContent}
              </div>
            </>,
            document.body,
          )
        : null}
    </div>
  );
}

/** The project a `Project: Previous` / `Project: Next` step is heading to. */
let projectStep: { to: string; direction: 1 | -1 } | null = null;

/**
 * Notes which way a project step runs, so a step that wraps past the last
 * project to the first (or back) still slides on in that direction.
 */
export function noteProjectStep(to: string, delta: number) {
  projectStep = { to, direction: delta < 0 ? -1 : 1 };
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
  const t = useT();
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
          <IconButton label={`${t("New tab")} (${MOD}T)`} onClick={onNew}>
            <Plus className="size-3.5" strokeWidth={1.75} />
          </IconButton>
        ) : null}
        {onSearch ? (
          <IconButton
            label={`${t("Search")} (${MOD}K)`}
            active={searchActive}
            onClick={onSearch}
          >
            <Search className="size-3.5" strokeWidth={1.75} />
          </IconButton>
        ) : null}
        {onOpenInbox ? (
          <IconButton
            label={inboxUnseen ? t("Inbox, new items") : t("Inbox")}
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
          <IconButton
            label={t("Notes")}
            active={notesActive}
            onClick={onOpenNotes}
          >
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
  changeAdditions,
  changeDeletions,
  hasChanges,
  hasUnseenFinished,
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
  changeAdditions: number;
  changeDeletions: number;
  hasChanges: boolean;
  /** A session here finished a reply the user has not opened. */
  hasUnseenFinished: boolean;
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
  const railTooltip = useRailTooltip();

  return (
    <nav
      aria-label="Project shortcuts"
      data-compact-project-rail
      {...railTooltip.handlers}
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
          tooltip={false}
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
              label={
                itemId === "changes"
                  ? changesLabel
                  : itemId === "sessions" && hasUnseenFinished
                    ? "Sessions, new replies"
                    : TAB_LABELS[itemId]
              }
              tipLabel={TAB_LABELS[itemId]}
              diff={
                itemId === "changes"
                  ? { additions: changeAdditions, deletions: changeDeletions }
                  : undefined
              }
              icon={COMPACT_TAB_ICONS[itemId]}
              active={workspaceActive && tabShown && activeTab === itemId}
              dot={
                (itemId === "changes" && hasChanges) ||
                (itemId === "sessions" && hasUnseenFinished)
              }
              onClick={() => openWorkspaceTab(itemId)}
            />
          ))}
        </div>
        <CompactRailAction
          label="Search"
          shortcut={`${MOD}K`}
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
          label="Settings"
          shortcut={`${MOD},`}
          tooltip={false}
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
      {railTooltip.tooltip}
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
      data-rail-tip={name}
      data-rail-tip-detail={
        unseen
          ? `${MONO_STATUS_LABEL[status]} · ${unreadLabel(unseen)}`
          : MONO_STATUS_LABEL[status]
      }
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
  shortcut,
  tooltip = true,
  tipLabel = label,
  diff,
  icon: Icon,
  tab = false,
  active = false,
  dot = false,
  onClick,
  onOpenContextMenu,
}: {
  label: string;
  shortcut?: string;
  tooltip?: boolean;
  /** The tooltip's name, when the accessible label carries more. */
  tipLabel?: string;
  /** Colored +n -n beside the tooltip's name. */
  diff?: { additions: number; deletions: number };
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
      data-rail-tip={tooltip ? tipLabel : undefined}
      data-rail-tip-detail={tooltip ? shortcut : undefined}
      data-rail-tip-additions={
        tooltip && diff?.additions ? diff.additions : undefined
      }
      data-rail-tip-deletions={
        tooltip && diff?.deletions ? diff.deletions : undefined
      }
      aria-label={shortcut ? `${label} (${shortcut})` : label}
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

/** Zen-style spaces at the foot of the inset sidebar: each project's logo or
 * mascot, the current one lifted, and a way to open another. */
/** Projects the spaces bar shows at once; the chevrons page through more. */
const SPACES_SHOWN = 5;

function ProjectSpacesBar({
  projects,
  cwd,
  busyProjectPaths,
  onSelectProject,
  onProjectContextMenu,
}: {
  projects: readonly string[];
  cwd: string;
  busyProjectPaths?: Iterable<string>;
  onSelectProject: (path: string) => void;
  onProjectContextMenu: (
    path: string,
    x: number,
    y: number,
    trigger: HTMLElement,
  ) => void;
}) {
  const projectTooltip = useRailTooltip("top");
  const shortcuts = useSyncExternalStore(subscribeKeybindings, () =>
    [
      keybindingShortcutLabel("Project: Previous", `${MOD}${SHIFT}←`),
      keybindingShortcutLabel("Project: Next", `${MOD}${SHIFT}→`),
    ]
      .filter(Boolean)
      .join(" / "),
  );
  const shown = Math.min(projects.length, SPACES_SHOWN);
  const lastStart = Math.max(0, projects.length - shown);
  const activeIndex = projects.findIndex((path) => sameProjectPath(path, cwd));
  const [start, setStart] = useState(() =>
    Math.min(lastStart, Math.max(0, activeIndex - shown + 1)),
  );
  // Bring the current project into view when it changes from elsewhere,
  // moving the window no further than it needs to.
  useEffect(() => {
    if (activeIndex < 0) return;
    setStart((current) =>
      activeIndex < current
        ? activeIndex
        : activeIndex >= current + shown
          ? activeIndex - shown + 1
          : current,
    );
  }, [activeIndex, shown]);
  const first = Math.min(start, lastStart);
  const visible = projects.slice(first, first + shown);
  const rowRef = useRef<HTMLDivElement>(null);
  const step = (direction: -1 | 1) => {
    const next = Math.min(lastStart, Math.max(0, first + direction));
    if (next === first) return;
    setStart(next);
    // A short slide in from the side the new project comes from.
    const row = rowRef.current;
    if (
      !row ||
      typeof row.animate !== "function" ||
      window.matchMedia?.("(prefers-reduced-motion: reduce)").matches
    )
      return;
    row.animate(
      [
        { transform: `translateX(${direction * 12}px)`, opacity: 0.4 },
        { transform: "none", opacity: 1 },
      ],
      { duration: 200, easing: "cubic-bezier(0.32, 0.72, 0, 1)" },
    );
  };
  const chevron = (direction: -1 | 1) => {
    const disabled = direction < 0 ? first <= 0 : first >= lastStart;
    const Icon = direction < 0 ? ChevronLeft : ChevronRight;
    const label = direction < 0 ? "Previous projects" : "More projects";
    return (
      <button
        type="button"
        title={label}
        aria-label={label}
        disabled={disabled}
        data-tauri-drag-region="false"
        onClick={() => step(direction)}
        className="grid size-5 shrink-0 place-items-center rounded-md text-content/50 hover:bg-content/10 hover:text-content disabled:pointer-events-none disabled:opacity-30"
      >
        <Icon className="size-3.5" strokeWidth={1.75} />
      </button>
    );
  };
  return (
    // As tall as the usage footer beside it, so the two read as one strip.
    // A mini carousel: up to five spaces spread evenly, chevrons either side.
    // Its edges line up with the rows above, and its controls with the usage
    // footer's; each space takes an equal share.
    <div
      {...projectTooltip.handlers}
      className="flex h-[30px] shrink-0 items-center gap-1 px-2 pb-0.5"
    >
      {chevron(-1)}
      <div className="min-w-0 flex-1 overflow-hidden">
        <div
          ref={rowRef}
          role="tablist"
          aria-label="Projects"
          className="flex w-full items-center gap-1"
        >
          {visible.map((path) => (
            <ProjectSpaceButton
              key={path}
              path={path}
              active={sameProjectPath(path, cwd)}
              busy={projectPathBusy(busyProjectPaths, path)}
              shortcuts={shortcuts}
              onSelect={() => onSelectProject(path)}
              onContextMenu={(x, y, trigger) =>
                onProjectContextMenu(path, x, y, trigger)
              }
            />
          ))}
        </div>
      </div>
      {chevron(1)}
      {projectTooltip.tooltip}
    </div>
  );
}

function ProjectSpaceButton({
  path,
  active,
  busy,
  shortcuts,
  onSelect,
  onContextMenu,
}: {
  path: string;
  active: boolean;
  busy: boolean;
  shortcuts: string;
  onSelect: () => void;
  onContextMenu: (x: number, y: number, trigger: HTMLElement) => void;
}) {
  const label = useProjectLabel(path);
  return (
    <button
      type="button"
      role="tab"
      aria-selected={active}
      aria-label={busy ? `${label}, working` : label}
      data-rail-tip={label}
      data-rail-tip-detail={shortcuts || undefined}
      data-tauri-drag-region="false"
      onClick={active ? undefined : onSelect}
      onContextMenu={(event) => {
        event.preventDefault();
        event.stopPropagation();
        event.currentTarget.focus();
        onContextMenu(event.clientX, event.clientY, event.currentTarget);
      }}
      onKeyDown={(event) => {
        if (
          event.key !== "ContextMenu" &&
          !(event.shiftKey && event.key === "F10")
        )
          return;
        event.preventDefault();
        event.stopPropagation();
        const rect = event.currentTarget.getBoundingClientRect();
        onContextMenu(rect.left, rect.bottom, event.currentTarget);
      }}
      className={`relative grid h-5 min-w-0 flex-1 place-items-center rounded-md transition-[opacity,background-color] duration-200 ease-out motion-reduce:transition-none ${
        active
          ? "bg-content/10 opacity-100"
          : "opacity-45 hover:bg-content/5 hover:opacity-100"
      }`}
    >
      <CurrentProjectIcon
        cwd={path}
        busy={busy}
        className="size-3"
        mascotClassName="size-2.5"
      />
      {busy && !active ? (
        <span
          aria-hidden
          className="absolute right-1 top-0.5 size-1.5 rounded-full bg-accent"
        />
      ) : null}
    </button>
  );
}

/** The inset layout's project tools, beside the card. */
function RightPanel({
  tab,
  tabs,
  onReorder,
  changesLabel,
  hasChangeStats,
  changeAdditions,
  changeDeletions,
  onTabChange,
  children,
}: {
  tab: RightPanelTab;
  tabs: RightPanelTab[];
  onReorder: (tabs: RightPanelTab[]) => void;
  changesLabel: string;
  hasChangeStats: boolean;
  changeAdditions: number;
  changeDeletions: number;
  onTabChange: (tab: RightPanelTab) => void;
  children: ReactNode;
}) {
  const sortable = useAnimatedReorder(tabs, onReorder);
  return (
    <div className="flex h-full min-h-0 w-full flex-col">
      <div
        role="tablist"
        aria-label="Project"
        className="flex h-9 shrink-0 items-center gap-px border-b border-stroke px-2"
      >
        {tabs.map((itemId) => {
          const active = tab === itemId;
          const isChangesTab = itemId === "changes";
          return (
            <div
              key={itemId}
              ref={(el) => sortable.setItemRef(itemId, el)}
              className="reorder-item workspace-tab relative flex min-w-0 flex-1 touch-none items-stretch"
              onPointerDown={(event) => {
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
                  onTabChange(itemId);
                }}
                className={`flex h-6 min-w-0 flex-1 items-center justify-center self-center rounded-md px-2 text-[12px] leading-none ${
                  active ? "bg-selection text-content" : "text-content/50"
                }`}
              >
                {isChangesTab && hasChangeStats ? (
                  <DiffStat
                    additions={changeAdditions}
                    deletions={changeDeletions}
                  />
                ) : (
                  <span className="block truncate leading-label">
                    {TAB_LABELS[itemId]}
                  </span>
                )}
              </button>
            </div>
          );
        })}
      </div>
      {children}
    </div>
  );
}

const DAY_MS = 24 * 60 * 60 * 1000;

/** Which of the inset sidebar's date groups a session's last activity is in. */
function sessionAgeLabel(updatedAt: number, now: number): string {
  const today = new Date(now);
  today.setHours(0, 0, 0, 0);
  const startOfToday = today.getTime();
  if (updatedAt >= startOfToday) return "Today";
  if (updatedAt >= startOfToday - DAY_MS) return "Yesterday";
  if (updatedAt >= startOfToday - 6 * DAY_MS) return "Last 7 days";
  return "Older";
}

/** The session among `ids` that started `id` through the app CLI, if any. */
export function startedSessionCaller(
  id: string,
  ids: ReadonlySet<string>,
): string | undefined {
  if (!id.startsWith("app-")) return undefined;
  const rest = id.slice("app-".length);
  // The longest listed prefix, since IDs and request IDs both use hyphens.
  for (let index = rest.lastIndexOf("-"); index > 0;) {
    const candidate = rest.slice(0, index);
    if (candidate !== id && ids.has(candidate)) return candidate;
    index = rest.lastIndexOf("-", index - 1);
  }
  return undefined;
}

const NO_STARTED_SESSIONS: {
  byParent: ReadonlyMap<string, SessionSummary[]>;
  childIds: ReadonlySet<string>;
} = { byParent: new Map(), childIds: new Set() };

/**
 * Sessions started through the app CLI are named `app-<caller>-<request>`, so
 * each finds its caller among the listed sessions. One a started session
 * started in turn joins the same top-level session. Sessions whose caller is
 * not listed, such as a Mono's, stay where they are.
 */
export function groupStartedSessions<T extends { id: string }>(
  sessions: readonly T[],
): { byParent: ReadonlyMap<string, T[]>; childIds: ReadonlySet<string> } {
  const ids = new Set(sessions.map((session) => session.id));
  const callerOf = (id: string) => startedSessionCaller(id, ids);
  const byParent = new Map<string, T[]>();
  const childIds = new Set<string>();
  for (const session of sessions) {
    let root = callerOf(session.id);
    if (!root) continue;
    const seen = new Set([session.id]);
    for (let up = callerOf(root); up && !seen.has(up); up = callerOf(up)) {
      seen.add(root);
      root = up;
    }
    childIds.add(session.id);
    const list = byParent.get(root);
    if (list) list.push(session);
    else byParent.set(root, [session]);
  }
  return { byParent, childIds };
}

/** A quiet heading over a group in the inset sidebar, actions at its end. */
function SidebarSectionLabel({
  label,
  children,
}: {
  label: string;
  children?: ReactNode;
}) {
  return (
    <div className="flex h-7 shrink-0 items-center gap-1 px-2 text-[12px] font-medium text-content/50">
      <span className="min-w-0 flex-1 truncate">{label}</span>
      {/* Pulled out so a button's icon, not its padding, lines up with the
          rows' trailing edge (their padding and border). */}
      {children ? <span className="-mr-[5px] flex">{children}</span> : null}
    </div>
  );
}

type SidebarNavItem = {
  label: string;
  icon: IconComponent;
  shortcut?: string;
  active?: boolean;
  dot?: boolean;
  onClick: () => void;
  onOpenContextMenu?: (x: number, y: number) => void;
};

/** The inset sidebar's links to the app-wide views, under the project. */
function SidebarNavList({
  search,
  items,
}: {
  /** The project rail's search field, above the links. */
  search?: { active: boolean; onClick: () => void };
  items: readonly (SidebarNavItem | false | undefined)[];
}) {
  const shown = items.filter((item): item is SidebarNavItem => !!item);
  if (!search && !shown.length) return null;
  return (
    <nav
      aria-label="Views"
      className="flex shrink-0 flex-col gap-px px-2 pb-3 pt-1"
    >
      {/* The project rail's search and rows, so the two layouts read the
          same. */}
      {search ? (
        <>
          <RailSearch
            label="Search"
            icon={Search}
            onClick={search.onClick}
            active={search.active}
            shortcut={`${MOD}K`}
            ariaLabel={`Search (${MOD}K)`}
          />
          <div className="mt-0.5" />
        </>
      ) : null}
      {shown.map(
        ({ label, icon, shortcut, active, dot, onClick, onOpenContextMenu }) => (
          <RailAction
            key={label}
            label={label}
            icon={icon}
            shortcut={shortcut}
            active={active}
            dot={dot}
            ariaLabel={dot ? `${label}, new items` : undefined}
            onClick={onClick}
            onOpenContextMenu={onOpenContextMenu}
          />
        ),
      )}
    </nav>
  );
}

function WorkspaceTitleActions({
  onSearch,
  onNew,
}: {
  onSearch?: () => void;
  onNew?: () => void;
}) {
  const t = useT();
  if (!onSearch && !onNew) return null;
  return (
    <div
      className="flex shrink-0 items-center gap-0.5"
      data-tauri-drag-region="false"
    >
      {onSearch ? (
        <IconButton label={`${t("Go to File")} (${MOD}P)`} onClick={onSearch}>
          <Search className="size-3.5" strokeWidth={1.75} />
        </IconButton>
      ) : null}
      {onNew ? (
        <IconButton label={`${t("New session")} (${MOD}T)`} onClick={onNew}>
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
  label = false,
  dense = false,
  onPointerDown,
  onToggle,
  onContextMenu,
  onRename,
}: {
  folder: Pick<SessionFolder, "name" | "colorIndex" | "customColor">;
  /** A section label, as in the inset sidebar: the name and a caret. */
  label?: boolean;
  /** Sized like the inset sidebar's session cards. */
  dense?: boolean;
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
  if (label)
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
        className={`relative flex h-7 w-full touch-none items-center gap-1.5 rounded-md px-2 text-left text-[12px] font-medium text-content/50 hover:text-content/80 ${
          dropTarget ? "text-content" : ""
        }`}
      >
        {dropTarget ? (
          <div className="pointer-events-none absolute inset-0 rounded-md bg-accent/20" />
        ) : null}
        {accent ? (
          <span
            aria-hidden
            className="relative size-1.5 shrink-0 rounded-full"
            style={{ background: accent }}
          />
        ) : null}
        <span className="relative min-w-0 truncate">{folder.name}</span>
        <ChevronDown
          className={`relative size-3 shrink-0 transition-transform motion-reduce:transition-none ${
            expanded ? "" : "-rotate-90"
          }`}
          strokeWidth={2}
        />
        <span className="relative ml-auto flex shrink-0 items-center gap-1 text-[11px] tabular-nums">
          {!expanded && needsApproval ? (
            <CircleAlert className="size-3 text-amber-400" strokeWidth={1.75} />
          ) : !expanded && busy ? (
            <TerminalSpinner className="inline-block w-3 select-none text-center text-[11px] leading-none text-accent" />
          ) : !expanded && done ? (
            <Check className="size-3 text-emerald-400" strokeWidth={2.25} />
          ) : null}
          {expanded ? null : <span>{count}</span>}
        </span>
      </button>
    );
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
      className={`group relative flex w-full touch-none items-center ${
        dense ? "gap-2" : "gap-1.5"
      } px-2 h-8 text-left ${expanded || dense ? "rounded-md" : ""} ${
        dropTarget
          ? "text-content"
          : dense
            ? // Muted like the links above it, Search through Automations.
              "text-content/50 hover:bg-content/10 hover:text-content"
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
          dense ? "opacity-70" : accent ? "" : "text-content/50"
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
                className={`hidden ${dense ? "size-4" : "size-3.5 text-content"} group-hover:block group-focus-visible:block`}
                strokeWidth={1.75}
              />
            </>
          ) : (
            <ChevronDown
              className={dense ? "size-4" : "size-3.5 text-content"}
              strokeWidth={1.75}
            />
          )
        ) : (
          <>
            <span className="group-hover:hidden group-focus-visible:hidden">
              {groupIcon ?? (
                <Folder
                  className={dense ? "size-4" : "size-3.5 text-content"}
                  strokeWidth={1.75}
                />
              )}
            </span>
            <ChevronRight
              className={`hidden ${dense ? "size-4" : "size-3.5 text-content"} group-hover:block group-focus-visible:block`}
              strokeWidth={1.75}
            />
          </>
        )}
      </span>
      <span
        className={`relative min-w-0 flex-1 truncate ${
          dense
            ? "text-sm font-medium leading-tight"
            : "text-[13px] font-semibold leading-snug text-content"
        }`}
      >
        {folder.name}
      </span>
      <span
        className={`relative flex shrink-0 items-center gap-1 text-[11px] tabular-nums ${
          dense ? "text-content/40" : "text-content/45"
        }`}
      >
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

export const SessionCard = memo(function SessionCard({
  session,
  isActive,
  isSelected,
  busy,
  done,
  linkedUpdate,
  needsApproval,
  dropTarget,
  compact = false,
  dense = false,
  nested = false,
  startedCount = 0,
  startedOpen = false,
  startedInView = false,
  modelInFooter = false,
  statusLabel,
  project,
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
  /** One line: status dot, provider mark, title and time, for the inset
   * layout's list. */
  dense?: boolean;
  /** A session another one started, on the rail under it: a smaller, muted
   * title like an orchestrator's agent rows. */
  nested?: boolean;
  /** How many listed sessions this one started through /operator. */
  startedCount?: number;
  /** Whether those sessions are showing on the rail below. */
  startedOpen?: boolean;
  /** One of those sessions is the one in view or chosen, so this row keeps
   * its hover fill as the group's head. */
  startedInView?: boolean;
  /** Shows the model as its provider's mark beside the branch, so the title
   * shares its row with the time and the card is a line shorter. */
  modelInFooter?: boolean;
  /** Stands in for the time, for a session with no state of its own to show. */
  statusLabel?: string;
  /** Names its project before the branch, where sessions span several. */
  project?: string;
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
  const t = useT();
  const skipClickUntil = useRef(0);
  const prefetchTimer = useRef<number | null>(null);
  const previewAnchor = useRef<HTMLDivElement>(null);
  const previewCloseTimer = useRef<number | null>(null);
  const previewId = useId();
  const [previewOpen, setPreviewOpen] = useState(false);
  const orchestrationTooltipRootRef = useRef<HTMLDivElement>(null);
  const orchestrationTooltipId = useId();
  const [dragging, setDragging] = useState(false);
  const [orchestrationTooltipOpen, setOrchestrationTooltipOpen] =
    useState(false);
  const cancelPreviewClose = useCallback(() => {
    if (previewCloseTimer.current != null) {
      window.clearTimeout(previewCloseTimer.current);
      previewCloseTimer.current = null;
    }
  }, []);
  const closePreview = useCallback(() => {
    cancelPreviewClose();
    setPreviewOpen(false);
  }, [cancelPreviewClose]);
  const showPreview = () => {
    cancelPreviewClose();
    if (dense && !dragging) setPreviewOpen(true);
  };
  const schedulePreviewClose = () => {
    cancelPreviewClose();
    // Let the pointer cross the gap into the preview without closing it.
    previewCloseTimer.current = window.setTimeout(closePreview, 120);
  };
  useEffect(() => cancelPreviewClose, [cancelPreviewClose]);
  const orchestration = session.orchestration;
  const draft = !!session.draft;
  const orchestrationExpanded =
    !!orchestration && (isActive || isSelected || busy);
  // An orchestrator or /operator thread opens a rail of work below it.
  const expandable =
    dense &&
    !nested &&
    ((!!orchestration && !orchestrationExpanded) ||
      (startedCount > 0 && !startedOpen));
  const markHover = expandable ? " group-hover:hidden" : "";
  // Marked on the right like a linked issue or PR, in its mode's colour.
  const modeStyle = orchestration
    ? MODE_COMMAND_STYLES[ORCHESTRATOR_COMMAND.name]
    : startedCount > 0
      ? MODE_COMMAND_STYLES[OPERATOR_COMMAND.name]
      : undefined;
  const modeLabel = orchestration
    ? `Orchestrator, ${orchestration.tasks.length} ${
        orchestration.tasks.length === 1 ? "agent" : "agents"
      }`
    : `Operator, started ${startedCount} ${
        startedCount === 1 ? "session" : "sessions"
      }`;
  const modeBadge =
    dense && !nested && modeStyle ? (
      <span
        role="img"
        title={modeLabel}
        aria-label={modeLabel}
        data-session-mode={orchestration ? "orchestrator" : "operator"}
        className={`grid size-4 shrink-0 place-items-center ${modeStyle.className}`}
      >
        <modeStyle.Icon className="size-3" strokeWidth={1.75} />
      </span>
    ) : null;
  // Set once the card has painted, so only later changes animate.
  const cardShown = useRef(false);
  useEffect(() => {
    cardShown.current = true;
  }, []);
  const orchestrationDone =
    orchestration?.tasks.filter((task) => task.status === "completed").length ??
    0;
  const title = sessionDisplayTitle(session.title, session.harness);
  const gitLabel = session.worktreeRemoved
    ? NO_BRANCH_LABEL
    : formatGitLabel(session.repo, session.branch);
  const time = formatRelative(session.updatedAt, now);
  const headerRow = !(compact && !orchestrationExpanded) && !modelInFooter;
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
          <span>
            {orchestration ? t("Needs input") : t("Need approval")}
          </span>
        </>
      ) : busy ? (
        <>
          <TerminalSpinner className="inline-block w-3 select-none text-center text-[11px] leading-none text-accent" />
          <span>{t("Working...")}</span>
        </>
      ) : done ? (
        <>
          <Check className="size-3" strokeWidth={2.25} />
          <span>{t("Done")}</span>
        </>
      ) : draft ? (
        <>
          <CircleDashed className="size-3" strokeWidth={1.75} />
          <span>Draft</span>
        </>
      ) : (
        <span>{statusLabel ?? time}</span>
      )}
    </span>
  );

  const linkedWorkItem = session.linkedWorkItem;
  const linkedUpdateDot = linkedUpdate ? (
    <span
      title={t("Linked {kind} updated since this session", {
        kind: linkedWorkItem?.kind === "pr" ? "PR" : t("issue"),
      })}
      aria-label={t("Linked work item updated")}
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
      {dense ? null : <span>#{linkedWorkItem.number}</span>}
    </button>
  ) : null;

  const onKeyDown = (e: ReactKeyboardEvent<HTMLDivElement>) => {
    if (e.target !== e.currentTarget) return;
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      closePreview();
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

  const archiveLabel = session.archived ? t("Unarchive") : t("Archive");
  // Expanding an orchestration card must not move its existing header. Keep
  // the collapsed top inset and give only the new detail area extra room at
  // the bottom.
  // Dense rows match the project rail's 32px rows.
  // Dense rows hang their agents on a rail below the card instead.
  const cardPaddingY = dense
    ? "py-[3px]"
    : orchestrationExpanded
      ? compact
        ? "pb-2.5 pt-1.5"
        : "pb-2.5 pt-2"
      : compact
        ? "py-1.5"
        : "py-2";

  return (
    <>
      <div className="group relative">
        <div
          ref={previewAnchor}
          title={dense ? undefined : title}
          data-session-card={session.id}
          data-orchestration-card={orchestration ? "true" : undefined}
          data-session-selected={isSelected ? "true" : undefined}
          data-tauri-drag-region="false"
          onPointerDown={onPointerDown}
          onPointerDownCapture={closePreview}
          onPointerEnter={(event) => {
            schedulePrefetch();
            if (event.pointerType !== "touch") showPreview();
          }}
          onPointerLeave={() => {
            cancelScheduledPrefetch();
            if (previewOpen) schedulePreviewClose();
          }}
          onClick={(event) => {
            closePreview();
            if (performance.now() < skipClickUntil.current) return;
            onSelect(session.id, event);
          }}
          onContextMenu={(event) => {
            closePreview();
            onContextMenu?.(session.id, event);
          }}
          className={`relative border flex w-full cursor-default select-none touch-none flex-col rounded-md ${nested ? "px-1.5" : dense ? "px-2" : "px-2.5"} text-left ${cardPaddingY} ${
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
                          orchestrationExpanded && !dense
                            ? "bg-content/5 hover:bg-content/10"
                            : startedInView
                              ? "bg-content/5"
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
            aria-describedby={dense && previewOpen ? previewId : undefined}
            onFocus={(event) => {
              if (event.currentTarget.matches(":focus-visible")) showPreview();
            }}
            onBlur={closePreview}
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
            {dense ? (
              <span
                className={`relative flex min-w-0 items-center ${
                  nested ? "h-5 gap-1.5" : "h-6 gap-2"
                }`}
              >
                {/* A collapsed orchestrator or /operator thread shows a
                    chevron in its mark's place on hover: selecting it opens
                    the rail below. */}
                {expandable ? (
                  <span className="hidden size-4 shrink-0 place-items-center text-content/60 group-hover:grid">
                    <ChevronDown className="size-3.5" strokeWidth={1.75} />
                  </span>
                ) : null}
                {/* A working session spins in place of its provider's mark,
                  then shows a check until it is seen. */}
                {busy ? (
                  <span
                    className={`grid size-4 shrink-0 place-items-center${markHover}`}
                  >
                    <TerminalSpinner
                      aria-label="Working"
                      className="inline-block w-3.5 select-none text-center text-[11px] leading-none text-accent"
                    />
                  </span>
                ) : done ? (
                  <span
                    className={`grid size-4 shrink-0 place-items-center${markHover}`}
                  >
                    <Check
                      role="img"
                      aria-label="Done"
                      className="size-3.5 text-emerald-400"
                      strokeWidth={2.25}
                    />
                  </span>
                ) : (
                  <HarnessIcon
                    harness={session.harness}
                    className={`${nested ? "size-3.5" : "size-4"} shrink-0${markHover}`}
                  />
                )}
                {session.pinned ? (
                  <Pin
                    className="-mx-0.5 size-3.5 shrink-0 text-content/45"
                    strokeWidth={1.75}
                  />
                ) : null}
                <ParticleText
                  text={title}
                  className={`line-clamp-1 min-w-0 flex-1 leading-tight ${
                    nested
                      ? `text-[13px] ${
                          isActive || isSelected
                            ? "text-content"
                            : "text-content/55 group-hover:text-content/80"
                        }`
                      : "text-sm font-medium text-content"
                  }`}
                />
                <span className="flex shrink-0 items-center gap-1">
                  {linkedUpdateDot}
                  {workItemBadge}
                  {modeBadge}
                  {session.automationId ? (
                    <span
                      data-automation-icon
                      role="img"
                      title="Started by an automation"
                      aria-label="Started by an automation"
                      className="grid size-4 shrink-0 place-items-center text-amber-400"
                    >
                      <Zap className="size-3" strokeWidth={1.75} />
                    </span>
                  ) : null}
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
                      // Pulled right so its box sits as far from the card's
                      // right edge as from its top.
                      className="-mr-[3px] hidden size-5 place-items-center rounded-md text-content/50 hover:bg-content/10 hover:text-content group-focus-within:grid group-hover:grid"
                    >
                      <Archive className="size-3 shrink-0" strokeWidth={1.75} />
                    </button>
                  ) : null}
                  {/* A state worth noticing takes the time's place, as an icon
                    so the row keeps its alignment. */}
                  <span
                    className={`flex items-center text-[11px] tabular-nums text-content/40 ${
                      onArchive
                        ? "group-focus-within:hidden group-hover:hidden"
                        : ""
                    }`}
                  >
                    {needsApproval ? (
                      <CircleAlert
                        role="img"
                        aria-label={
                          orchestration ? "Needs input" : "Needs approval"
                        }
                        className="size-3.5 text-amber-400"
                        strokeWidth={1.75}
                      />
                    ) : draft ? (
                      <CircleDashed
                        role="img"
                        aria-label="Draft"
                        className="size-3.5"
                        strokeWidth={1.75}
                      />
                    ) : (
                      (statusLabel ?? time)
                    )}
                  </span>
                </span>
              </span>
            ) : null}
            {!dense && headerRow ? (
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
            ) : null}
            {dense ? null : (
              <span
                className={`relative flex min-w-0 items-center gap-1.5 ${
                  headerRow ? "mt-1" : ""
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
                {headerRow ? null : (
                  <span className="ml-auto flex shrink-0 items-center gap-1.5 pl-2">
                    {linkedUpdateDot}
                    {status}
                  </span>
                )}
              </span>
            )}
          </div>
          {orchestrationExpanded && !dense ? (
            <OrchestrationSidebarAgents
              leadId={session.id}
              summary={orchestration!}
            />
          ) : null}
          {dense ? null : (
            <span className="relative mt-1 flex items-center gap-2">
              {project && !gitLabel ? (
                <span
                  className="flex min-w-0 flex-1 items-center gap-1 text-[11px] text-content/45"
                  title={session.cwd}
                >
                  <Folder className="size-3 shrink-0" strokeWidth={1.75} />
                  <span className="min-w-0 truncate">{project}</span>
                </span>
              ) : gitLabel ? (
                <span
                  className="flex min-w-0 flex-1 items-center gap-1 text-[11px] text-content/45"
                  title={
                    session.worktreeCwd
                      ? `${gitLabel}\n${session.worktreeCwd}`
                      : gitLabel
                  }
                >
                  <GitBranch className="size-3 shrink-0" strokeWidth={1.75} />
                  <span className="min-w-0 truncate">
                    {project ? `${project} · ${gitLabel}` : gitLabel}
                  </span>
                </span>
              ) : (
                <span className="min-w-0 flex-1" />
              )}
              <span className="relative flex shrink-0 items-center gap-px">
                {/* Just the provider's mark; the model's name slides out beside
                it while the card is hovered or focused. */}
                {modelInFooter && model ? (
                  <span
                    title={model}
                    className="flex min-w-0 items-center text-[11px] text-content/50"
                  >
                    <span className="max-w-0 truncate pr-0 opacity-0 transition-[max-width,opacity,padding] duration-200 ease-out group-focus-within:max-w-32 group-focus-within:pr-1.5 group-focus-within:opacity-100 group-hover:max-w-32 group-hover:pr-1.5 group-hover:opacity-100 motion-reduce:transition-none">
                      {model}
                    </span>
                    <HarnessIcon
                      harness={session.harness}
                      className="size-3.5 shrink-0"
                    />
                  </span>
                ) : null}
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
                      aria-label={t(
                        "Orchestrator, {count} {unit}, {done} done",
                        {
                          count: orchestration.tasks.length,
                          unit: t(
                            orchestration.tasks.length === 1
                              ? "subagent"
                              : "subagents",
                          ),
                          done: orchestrationDone,
                        },
                      )}
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
          )}
        </div>
        {dense && previewOpen && previewAnchor.current ? (
          <SessionCardPreview
            anchor={previewAnchor.current}
            id={previewId}
            session={session}
            status={status}
            onClose={closePreview}
            onPointerEnter={cancelPreviewClose}
            onPointerLeave={schedulePreviewClose}
          />
        ) : null}
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
                {t("Subagents")}
              </span>
              <span className="shrink-0 text-[10px] tabular-nums text-content/45">
                {t("{done}/{total} done", {
                  done: orchestrationDone,
                  total: orchestration.tasks.length,
                })}
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
                      {/* `label` stays the english source: it doubles as the
                          status token compared above. */}
                      {t(label)}
                    </span>
                  </div>
                );
              })}
            </div>
          </Popover>
        ) : null}
      </div>
      {/* Under the row, outside its highlight and its hover actions, like a
          tool step's rail. */}
      {orchestration && dense ? (
        // Opening under a row already on screen grows in and pushes the
        // list down; one the list loads with is simply there.
        <RevealHeight
          open={orchestrationExpanded}
          animateIn={cardShown.current}
        >
          {/* One item, padding and all, for the reveal to grow. */}
          <div>
            <OrchestrationSidebarAgents
              leadId={session.id}
              summary={orchestration}
              variant="tree"
            />
          </div>
        </RevealHeight>
      ) : null}
    </>
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
  const hasChanges = additions > 0 || deletions > 0;

  useLayoutEffect(() => {
    const container = containerRef.current;
    const content = contentRef.current;
    if (!container || !content) return;

    let availableWidth = 0;
    let textWidth = 0;
    let fontSize = parseFloat(content.style.fontSize) || 11;
    let frame = 0;
    // Observe both boxes after layout. Repeated font writes and synchronous
    // measurements here used to force whole-workspace layout on navigation.
    // The gap uses em units, so the complete label scales with its font size.
    const observer = new ResizeObserver((entries) => {
      for (const entry of entries) {
        if (entry.target === container)
          availableWidth = entry.contentRect.width;
        else textWidth = entry.contentRect.width;
      }
      if (!availableWidth || !textWidth || frame) return;
      frame = requestAnimationFrame(() => {
        frame = 0;
        const next = Math.max(
          0.1,
          Math.min(
            11,
            Math.floor((fontSize * availableWidth * 10) / textWidth) / 10,
          ),
        );
        if (next === fontSize) return;
        content.style.fontSize = `${next}px`;
        fontSize = next;
      });
    });
    observer.observe(container);
    observer.observe(content);
    return () => {
      observer.disconnect();
      if (frame) cancelAnimationFrame(frame);
    };
  }, [hasChanges]);

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
