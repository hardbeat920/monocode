import {
  Check,
  ChevronDown,
  ChevronRight,
  CircleAlert,
  Folder,
  GitBranch,
  Inbox,
  ListFilter,
  Pin,
  Plus,
  Search,
  Settings,
  StickyNote,
} from "./icons";
import {
  memo,
  useEffect,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type MouseEvent as ReactMouseEvent,
  type PointerEvent as ReactPointerEvent,
  type ReactNode,
} from "react";
import {
  loadSidebarTabOrder,
  saveSidebarTabOrder,
  type SidebarTabId,
} from "../lib/appearance";
import { basename, type GitHistoryCommit } from "../lib/fs";
import { IS_MAC, MOD } from "../lib/platform";
import { resolveModel } from "../lib/models";
import { projectName } from "../lib/paths";
import { sessionDisplayTitle } from "../lib/session";
import { nextUnseenFinishedSessions } from "../lib/sessionDone";
import { paneDropFromPoint, setExternalPaneDrop } from "../lib/paneDrop";
import type { PaneEdge } from "../lib/layout";
import { suppressTextSelection } from "../lib/drag";
import {
  compareSessionSummaries,
  filterSessionsByArchive,
  filterSessionsByQuery,
} from "../lib/sessionHistory";
import {
  addSessionToFolder,
  applySessionListDrop,
  buildSessionList,
  createFolderWithSessions,
  dissolveFolder,
  folderAccent,
  folderContaining,
  folderShellFill,
  loadSessionFolders,
  mergeFolderSessionSummaries,
  pruneSessionFolders,
  removeSessionFromFolder,
  renameFolder,
  reorderSessionFolders,
  saveSessionFolders,
  setFolderCollapsed,
  setFolderColor,
  setFolderCustomColor,
  ungroupedSessions,
  type SessionFolder,
  type SessionListDropTarget,
} from "../lib/sessionFolders";
import { SESSION_LIST_PAGE, sessionListWindow } from "../lib/sessionListWindow";
import {
  filterSessionsByHarness,
  filterSessionsByStatus,
  filterSessionsByTime,
  harnessesInSessions,
  hasActiveSessionFilters,
  loadSessionSidebarFilters,
  saveSessionSidebarFilters,
  type SessionSidebarFilters,
} from "../lib/sessionFilters";
import type { HarnessId } from "../lib/session";
import type { LiveAgent } from "../lib/liveAgents";
import type { SessionSummary } from "../lib/sessionStore";
import type { SettingsSectionId } from "../lib/settings";
import type { InstalledUpdate } from "../lib/updateNotice";
import {
  loadTabGroupColors,
  loadTabGroupCustomColors,
  loadTabGroupLabels,
  loadTabGroupMascots,
  resolveTabGroupColor,
  resolveTabGroupLabel,
  resolveTabGroupLogo,
  resolveTabGroupMascot,
  TAB_GROUP_COLORS,
} from "../lib/tabGroups";
import { useDragResize } from "../hooks/useDragResize";
import { useGitFileStatuses } from "../hooks/useGitFileStatuses";
import { useInboxUnseen } from "../hooks/useInboxUnseen";
import { useLockOverscroll } from "../hooks/useLockOverscroll";
import { useProjectDiffStats } from "../hooks/useProjectDiffStats";
import { useSortable } from "../hooks/useSortable";
import { useTabGroupLogos } from "../hooks/useTabGroupLogos";
import { normalizeHex } from "../lib/colorUtils";
import {
  looksLikeProject,
  sameProjectPath,
  type RecentProject,
} from "../lib/recents";
import { CwdPicker } from "./CwdPicker";
import { ColorPickerPopover, ColorSwatchRow } from "./ColorPickerPopover";
import { ExplorerMenu, type ExplorerMenuItem } from "./ExplorerMenu";
import { FileTree } from "./FileTree";
import { HarnessIcon } from "./HarnessIcon";
import { ProjectRail } from "./ProjectRail";
import { RailAction } from "./RailAction";
import { TerminalSpinner } from "./TerminalSpinner";
import { DevModeSlot, IconButton, TabVisitNav } from "./TitleBar";
import { ProjectSearch } from "./ProjectSearch";
import { ProjectLogoIcon } from "./ProjectLogoIcon";
import { ProjectMascot } from "./ProjectMascot";
import { SessionFiltersMenu } from "./SessionFiltersMenu";
import { SessionsEmpty } from "./SessionsEmpty";
import { SidebarUpdateFooter } from "./SidebarUpdate";
import { SourceControl } from "./SourceControl";

const MIN_WIDTH = 260;
const MAX_WIDTH = 560;
const DEFAULT_WIDTH = 260;

let rememberedWidth = DEFAULT_WIDTH;

type SidebarTab = SidebarTabId;

const TAB_LABELS: Record<SidebarTab, string> = {
  sessions: "Sessions",
  inbox: "Inbox",
  files: "Explorer",
  changes: "Changes",
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
  onPrefetchSession?: (sessionId: string) => void;
  onPlaceSessionOnPane?: (
    sessionId: string,
    targetId: string,
    edge: PaneEdge,
  ) => void;
  onRenameSession?: (sessionId: string, title: string) => void;
  onArchiveSession?: (sessionId: string, archived: boolean) => void;
  onPinSession?: (sessionId: string, pinned: boolean) => void;
  onDeleteSession?: (sessionId: string) => void;
  onOpenFile: (path: string) => void;
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
  onOpenDiff?: (path: string) => void;
  onOpenCommit?: (commit: GitHistoryCommit) => void;
  selectedDiffPath?: string;
  selectedCommitSha?: string;
  textHarness?: HarnessId;
  onShowSourceControl?: () => void;
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
  onOpenNotes?: () => void;
  onGoToFile?: () => void;
  searchActive?: boolean;
  inboxActive?: boolean;
  notesActive?: boolean;
  notesEnabled?: boolean;
  onToggleProjectRail?: () => void;
  projectRailOpen?: boolean;
  unseenFinishedIds?: Set<string>;
  settingsOpen?: boolean;
  settingsSection?: SettingsSectionId;
  onOpenSettings?: () => void;
  onSelectSettingsSection?: (section: SettingsSectionId) => void;
  onCloseSettings?: () => void;
  updateNotice?: InstalledUpdate | null;
  onOpenWhatsNew?: (version: string) => void;
  onDismissUpdate?: () => void;
};

function SidebarComponent({
  cwd,
  gitCwd,
  open,
  sessions,
  busySessionIds,
  approvalSessionIds,
  activeSessionId,
  openSessions = [],
  status,
  pending,
  onSelectSession,
  onPrefetchSession,
  onPlaceSessionOnPane,
  onRenameSession,
  onArchiveSession,
  onPinSession,
  onDeleteSession,
  onOpenFile,
  onOpenTerminal,
  onFileMoved,
  onFileDeleted,
  tab,
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
  onOpenCommit,
  selectedDiffPath,
  selectedCommitSha,
  textHarness,
  onShowSourceControl,
  recents = [],
  busyProjectPaths,
  liveAgents = [],
  onSelectAgent,
  onSelectProject,
  onOpenProject,
  onRemoveProject,
  onNew,
  onNewTerminal,
  onSearch,
  onOpenInbox,
  onOpenNotes,
  onGoToFile,
  searchActive = false,
  inboxActive = false,
  notesActive = false,
  notesEnabled = true,
  onToggleProjectRail,
  projectRailOpen = true,
  unseenFinishedIds: unseenFinishedIdsProp,
  settingsOpen = false,
  settingsSection = "general",
  onOpenSettings,
  onSelectSettingsSection,
  onCloseSettings,
  updateNotice = null,
  onOpenWhatsNew,
  onDismissUpdate,
}: Props) {
  const gitRoot = gitCwd || cwd;
  const inboxUnseen = useInboxUnseen(recents, cwd);
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
  const [sessionListLimit, setSessionListLimit] = useState(SESSION_LIST_PAGE);
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
  const pendingFirstLoad = pending && sessions.length === 0;
  const listedSessions = mergeFolderSessionSummaries(
    sessions,
    openSessions,
    sessionFolders,
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
        busySessionIds,
        approvalSessionIds,
        unseenFinishedIds,
      ),
      searchQuery,
    ),
  ].sort(compareSessionSummaries);
  // Summaries for the whole project stay in `sessions` so filters still work.
  // Folders sit above the ungrouped list. Only a page of ungrouped cards
  // mounts; the sentinel below asks for the next page.
  const ungroupedVisible = ungroupedSessions(visibleSessions, sessionFolders);
  const activeUngroupedIndex = ungroupedVisible.findIndex(
    (session) => session.id === activeSessionId,
  );
  const shownUngroupedCount = sessionListWindow(
    ungroupedVisible.length,
    sessionListLimit,
    activeUngroupedIndex,
  );
  const shownUngrouped = ungroupedVisible.slice(0, shownUngroupedCount);
  const sessionListEntries = buildSessionList(
    visibleSessions,
    sessionFolders,
    shownUngrouped,
  );
  const hasMoreSessions = shownUngroupedCount < ungroupedVisible.length;
  const sessionListKey = `${cwd}\0${sessionFilters.showArchived}\0${sessionFilters.time}\0${sessionFilters.hiddenHarnesses.join(",")}\0${sessionFilters.status.working}\0${sessionFilters.status.needsApproval}\0${sessionFilters.status.done}\0${searchQuery}`;
  const sessionHarnesses = harnessesInSessions(sessions);
  const filtersActive = hasActiveSessionFilters(sessionFilters);
  const searchNarrowed = Boolean(searchQuery.trim());
  const narrowedByUser = searchNarrowed || filtersActive;
  const sortable = useSortable(tabOrder, (ids) => {
    const next = ids as SidebarTab[];
    setTabOrder(next);
    saveSidebarTabOrder(next);
    if (next[0]) onTabChange(next[0]);
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
  const visibleTabs = tabOrder.filter((itemId) => itemId !== "inbox");
  const canDragTabs = visibleTabs.length > 1;
  const showProjectRail = Boolean(onSelectProject && onOpenProject);
  // Settings live in the rail slot, so they keep it visible even when the
  // project rail itself is collapsed.
  const railVisible = showProjectRail && (projectRailOpen || settingsOpen);
  const inProject = looksLikeProject(cwd);
  const showSidebarFooter = !projectRailOpen;
  // A blank session has no project to browse, so the shell stands alone until
  // one is picked — whether or not the rail is open.
  const sidebarVisible =
    open &&
    !searchActive &&
    !inboxActive &&
    !notesActive &&
    !settingsOpen &&
    inProject;
  const gitStatuses = useGitFileStatuses(gitRoot, open && tab === "files");
  const changeStats = useProjectDiffStats(gitRoot, open);

  useEffect(() => {
    setSessionListLimit(SESSION_LIST_PAGE);
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
        setSessionListLimit((current) => current + SESSION_LIST_PAGE);
      },
      { root, rootMargin: "240px" },
    );
    observer.observe(sentinel);
    return () => observer.disconnect();
  }, [tab, hasMoreSessions, shownUngroupedCount]);

  useEffect(() => {
    setSessionFolders(loadSessionFolders(cwd));
    setRenamingFolderId(null);
    setFolderMenu(null);
    setSessionDrop(null);
    pendingFolderSessionIds.current.clear();
  }, [cwd]);

  useEffect(() => {
    if (pending || status === "error") return;
    const known = new Set(sessions.map((session) => session.id));
    for (const session of openSessions) known.add(session.id);
    if (activeSessionId) known.add(activeSessionId);
    for (const id of pendingFolderSessionIds.current) {
      known.add(id);
      if (
        sessions.some((session) => session.id === id) ||
        openSessions.some((session) => session.id === id)
      ) {
        pendingFolderSessionIds.current.delete(id);
      }
    }
    setSessionFolders((current) => {
      const next = pruneSessionFolders(current, known);
      if (next === current) return current;
      saveSessionFolders(cwd, next);
      return next;
    });
  }, [activeSessionId, cwd, openSessions, pending, sessions, status]);

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
      setSessionMenu(null);
      setFolderMenu(null);
      setFilterMenu(null);
    };
    const scrollParent = sessionsScrollRef.current ?? window;
    scrollParent.addEventListener("scroll", onScroll, true);
    return () => scrollParent.removeEventListener("scroll", onScroll, true);
  }, [sessionMenu, folderMenu, filterMenu]);

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

  const menuSession = sessionMenu
    ? sessions.find((session) => session.id === sessionMenu.sessionId)
    : undefined;
  const menuSessionFolder = sessionMenu
    ? folderContaining(sessionFolders, sessionMenu.sessionId)
    : undefined;
  const menuFolder = folderMenu
    ? sessionFolders.find((folder) => folder.id === folderMenu.folderId)
    : undefined;
  const folderMenuItems: ExplorerMenuItem[] = [
    { kind: "item", id: "rename", label: "Rename", shortcut: "F2" },
    { kind: "sep" },
    { kind: "item", id: "ungroup", label: "Ungroup" },
  ];
  const sessionMenuItems: ExplorerMenuItem[] = [
    ...(onPinSession
      ? [
          {
            kind: "item" as const,
            id: "pin",
            label: menuSession?.pinned ? "Unpin" : "Pin",
          },
        ]
      : []),
    ...(onRenameSession
      ? [
          {
            kind: "item" as const,
            id: "rename",
            label: "Rename",
            shortcut: "F2",
          },
        ]
      : []),
    { kind: "sep" as const },
    { kind: "item" as const, id: "folder-new", label: "New folder" },
    ...(sessionFolders.length > 0 ? [{ kind: "sep" as const }] : []),
    ...sessionFolders.map((folder) => ({
      kind: "item" as const,
      id: `folder-add:${folder.id}`,
      label: `Add to ${folder.name}`,
      checked: menuSessionFolder?.id === folder.id,
    })),
    ...(menuSessionFolder
      ? [
          {
            kind: "item" as const,
            id: "folder-remove",
            label: "Remove from folder",
          },
        ]
      : []),
    ...(onArchiveSession || onDeleteSession
      ? [
          { kind: "sep" as const },
          ...(onArchiveSession
            ? [
                {
                  kind: "item" as const,
                  id: "archive",
                  label: menuSession?.archived ? "Unarchive" : "Archive",
                },
              ]
            : []),
          ...(onDeleteSession
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
    e: ReactMouseEvent<HTMLButtonElement>,
  ) => {
    e.preventDefault();
    e.stopPropagation();
    setFilterMenu(null);
    setFolderMenu(null);
    setSessionMenu({ x: e.clientX, y: e.clientY, sessionId });
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
    const archived = !!menuSession?.archived;
    const pinned = !!menuSession?.pinned;
    setSessionMenu(null);
    if (id === "pin") {
      onPinSession?.(sessionId, !pinned);
      return;
    }
    if (id === "rename") {
      setRenamingSessionId(sessionId);
      return;
    }
    if (id === "folder-new") {
      const { folders, id: createdId } = createFolderWithSessions(
        sessionFolders,
        [sessionId],
      );
      if (!createdId) return;
      commitSessionFolders(folders);
      setRenamingFolderId(createdId);
      return;
    }
    if (id.startsWith("folder-add:")) {
      const folderId = id.slice("folder-add:".length);
      commitSessionFolders(
        setFolderCollapsed(
          addSessionToFolder(sessionFolders, folderId, sessionId),
          folderId,
          false,
        ),
      );
      return;
    }
    if (id === "folder-remove") {
      commitSessionFolders(removeSessionFromFolder(sessionFolders, sessionId));
      return;
    }
    if (id === "archive") {
      onArchiveSession?.(sessionId, !archived);
      return;
    }
    if (id === "delete") onDeleteSession?.(sessionId);
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

  const renderSessionCard = (session: SessionSummary, compact = false) =>
    renamingSessionId === session.id && onRenameSession ? (
      <SessionRenameRow
        session={session}
        isActive={session.id === activeSessionId}
        busy={busySessionIds.has(session.id)}
        needsApproval={approvalSessionIds.has(session.id)}
        onCommit={(title) => {
          onRenameSession(session.id, title);
          setRenamingSessionId(null);
        }}
        onCancel={() => setRenamingSessionId(null)}
      />
    ) : (
      <SessionCard
        session={session}
        isActive={session.id === activeSessionId}
        busy={busySessionIds.has(session.id)}
        done={unseenFinishedIds.has(session.id)}
        needsApproval={approvalSessionIds.has(session.id)}
        dropTarget={isSessionDrop("session", session.id)}
        compact={compact}
        now={now}
        onSelect={onSelectSession}
        onPrefetch={onPrefetchSession}
        onPlaceOnPane={onPlaceSessionOnPane}
        onListDrop={onSessionListDrop}
        onListDropTargetChange={setSessionDrop}
        onContextMenu={(e) => onSessionContextMenu(session.id, e)}
        onRename={
          onRenameSession ? () => setRenamingSessionId(session.id) : undefined
        }
        onDelete={
          onDeleteSession ? () => onDeleteSession(session.id) : undefined
        }
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

  const changeAdditions = changeStats?.additions ?? 0;
  const changeDeletions = changeStats?.deletions ?? 0;
  const hasChangeStats = changeAdditions > 0 || changeDeletions > 0;

  const workspaceTabItems = visibleTabs.map((itemId, index) => {
    const active = tab === itemId;
    const isChangesTab = itemId === "changes";
    const draggingTab = sortable.draggingId === itemId;
    const showStart =
      sortable.draggingId &&
      sortable.toIndex === index &&
      sortable.fromIndex !== null &&
      sortable.toIndex < sortable.fromIndex;
    const showEnd =
      sortable.draggingId &&
      sortable.toIndex === index &&
      sortable.fromIndex !== null &&
      sortable.toIndex > sortable.fromIndex;
    return (
      <div
        key={itemId}
        ref={(el) => sortable.setItemRef(itemId, el)}
        className={`relative flex min-w-0 flex-1 touch-none items-stretch ${
          draggingTab ? "opacity-40" : ""
        } ${canDragTabs ? "cursor-grab active:cursor-grabbing" : ""}`}
        onPointerDown={(event) => {
          if (event.button !== 0) return;
          onTabPick(itemId);
          sortable.onItemPointerDown(itemId, event);
        }}
      >
        {showStart ? (
          <div className="pointer-events-none absolute inset-y-0 left-0 z-20 w-0.5 bg-accent" />
        ) : null}
        {showEnd ? (
          <div className="pointer-events-none absolute inset-y-0 right-0 z-20 w-0.5 bg-accent" />
        ) : null}
        <button
          type="button"
          role="tab"
          aria-selected={active}
          aria-label={
            isChangesTab
              ? hasChangeStats
                ? [
                    "Changes",
                    changeAdditions > 0 ? `+${changeAdditions}` : "",
                    changeDeletions > 0 ? `-${changeDeletions}` : "",
                  ]
                    .filter(Boolean)
                    .join(" ")
                : "Changes"
              : undefined
          }
          data-tauri-drag-region="false"
          onClick={() => {
            if (sortable.consumeClick()) return;
            onTabPick(itemId);
          }}
          className={`flex h-6 min-w-0 flex-1 items-center justify-center self-center rounded-md px-2 text-[12px] leading-none ${
            active
              ? "bg-content/10 text-content"
              : "text-content/50 hover:bg-content/5 hover:text-content"
          } ${canDragTabs ? "cursor-grab active:cursor-grabbing" : ""}`}
        >
          {isChangesTab && hasChangeStats ? (
            <DiffStat additions={changeAdditions} deletions={changeDeletions} />
          ) : (
            <span className="block truncate">{TAB_LABELS[itemId]}</span>
          )}
        </button>
      </div>
    );
  });

  const sidebarContent = (
    <aside
      ref={resize.setPaneRef}
      className="sidebar-glass relative flex h-full min-h-0 shrink-0 flex-col border-r border-content/10"
    >
      {railVisible ? (
        <>
          <div
            className="flex h-10 shrink-0 select-none items-center gap-1 border-b border-content/10 pl-3 pr-1.5"
            data-tauri-drag-region="deep"
          >
            <span className="min-w-0 flex-1 truncate text-sm font-medium leading-tight">
              Workspace
            </span>
            <WorkspaceTitleActions onSearch={onGoToFile} onNew={onNew} />
          </div>
          <div
            role="tablist"
            aria-label="Workspace"
            className="flex h-9 shrink-0 items-center gap-px border-b border-content/10 px-2"
          >
            {workspaceTabItems}
          </div>
        </>
      ) : (
        <>
          <div
            className="flex h-10 shrink-0 select-none items-center border-b border-content/10 pr-1.5"
            data-tauri-drag-region="deep"
          >
            {IS_MAC ? <div className="w-[78px] shrink-0" /> : null}
            <DevModeSlot />
            <TabVisitNav
              canGoBack={canGoBack}
              canGoForward={canGoForward}
              onGoBack={onGoBack}
              onGoForward={onGoForward}
              onTogglePanel={onToggleProjectRail}
              panelActive={false}
            />
          </div>
          {onSelectProject ? (
            <SidebarProjectPicker
              cwd={cwd}
              recents={recents}
              busy={projectPathBusy(busyProjectPaths, cwd)}
              onSelectProject={onSelectProject}
              onNewTerminal={onNewTerminal}
              onSearch={onSearch}
              onOpenInbox={onOpenInbox}
              onOpenNotes={notesEnabled ? onOpenNotes : undefined}
              searchActive={searchActive}
              inboxActive={inboxActive}
              notesActive={notesActive}
              inboxUnseen={inboxUnseen}
            />
          ) : null}
          <div
            role="tablist"
            aria-label="Workspace"
            className="flex h-9 shrink-0 items-center gap-px overflow-visible border-b border-content/10 px-2"
          >
            {workspaceTabItems}
          </div>
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
              <FileTree
                key={gitRoot}
                cwd={gitRoot}
                onOpenFile={onOpenFile}
                onOpenTerminal={onOpenTerminal}
                onFileMoved={onFileMoved}
                onFileDeleted={onFileDeleted}
                onSearch={onOpenFilesSearch}
                gitStatuses={gitStatuses}
                sourceControlActive={open && tab === "changes"}
                onShowSourceControl={onShowSourceControl}
              />
            </div>
          ) : (
            <p className="px-3 py-2 text-[12px] text-content/50">
              No project folder
            </p>
          )}
        </div>
        {tab === "sessions" && cwd && cwd !== "~" ? (
          <div className="flex h-9 shrink-0 items-center gap-1 border-b border-content/10 px-2">
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
          className={`min-h-0 flex-1 overflow-y-auto overscroll-none ${
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
                sessions.length === 0 ? (
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
                ) : (
                  <SessionsEmpty message="Sessions you start will show up here" />
                )
              ) : (
                <ul className="flex flex-col gap-0.5 p-1.5">
                  {sessionListEntries.map((entry, index) => {
                    if (entry.kind === "divider") {
                      return (
                        <li
                          key={`divider-${index}`}
                          aria-hidden
                          className="mx-1 my-1 list-none"
                        >
                          <div className="h-px bg-content/10" />
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
                                canReorder={visibleFolderIds.length > 1}
                                busy={entry.sessions.some((session) =>
                                  busySessionIds.has(session.id),
                                )}
                                done={entry.sessions.some((session) =>
                                  unseenFinishedIds.has(session.id),
                                )}
                                needsApproval={entry.sessions.some((session) =>
                                  approvalSessionIds.has(session.id),
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
                                    <li key={session.id}>
                                      {renderSessionCard(session, true)}
                                    </li>
                                  ))}
                                </ul>
                                {onNew ? (
                                  <div className="border-t border-content/10 p-1">
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
                      <li key={entry.session.id}>
                        {renderSessionCard(entry.session)}
                      </li>
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
              enabled={open}
              textHarness={textHarness}
              selectedPath={selectedDiffPath}
              selectedSha={selectedCommitSha}
              onOpenFile={onOpenDiff ?? onOpenFile}
              onOpenCommit={onOpenCommit ?? (() => {})}
            />
          </div>
        ) : null}
        {showSidebarFooter ? (
          <>
            <SidebarUpdateFooter
              update={updateNotice}
              onOpenWhatsNew={onOpenWhatsNew}
              onDismissUpdate={onDismissUpdate}
            />
            <div className="flex shrink-0 flex-col gap-px p-2 pt-0">
              <RailAction
                label="Settings"
                icon={Settings}
                onClick={onOpenSettings}
                shortcut={`${MOD},`}
                ariaLabel={`Settings (${MOD},)`}
              />
            </div>
          </>
        ) : null}
      </>
      {sessionMenu ? (
        <ExplorerMenu
          x={sessionMenu.x}
          y={sessionMenu.y}
          items={sessionMenuItems}
          ariaLabel="Session actions"
          onPick={onSessionMenuPick}
          onClose={() => setSessionMenu(null)}
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
        railVisible || sidebarVisible ? "" : "hidden"
      }`}
    >
      {railVisible && onSelectProject && onOpenProject ? (
        <ProjectRail
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
          onTogglePanel={onToggleProjectRail}
          onSelectProject={onSelectProject}
          onOpenProject={onOpenProject}
          onRemoveProject={onRemoveProject}
          settingsOpen={settingsOpen}
          settingsSection={settingsSection}
          onOpenSettings={onOpenSettings}
          onSelectSettingsSection={onSelectSettingsSection}
          onCloseSettings={onCloseSettings}
          updateNotice={updateNotice}
          onOpenWhatsNew={onOpenWhatsNew}
          onDismissUpdate={onDismissUpdate}
        />
      ) : null}
      {sidebarVisible ? sidebarContent : null}
    </div>
  );
}

export const Sidebar = memo(SidebarComponent);

function SidebarProjectPicker({
  cwd,
  recents,
  busy,
  onSelectProject,
  onNewTerminal,
  onSearch,
  onOpenInbox,
  onOpenNotes,
  searchActive = false,
  inboxActive = false,
  notesActive = false,
  inboxUnseen = false,
}: {
  cwd: string;
  recents: RecentProject[];
  busy: boolean;
  onSelectProject: (path: string) => void;
  onNewTerminal?: () => void;
  onSearch?: () => void;
  onOpenInbox?: () => void;
  onOpenNotes?: () => void;
  searchActive?: boolean;
  inboxActive?: boolean;
  notesActive?: boolean;
  inboxUnseen?: boolean;
}) {
  const [groupLabels] = useState(loadTabGroupLabels);
  const [groupColors] = useState(loadTabGroupColors);
  const [groupCustomColors] = useState(loadTabGroupCustomColors);
  const [groupMascots] = useState(loadTabGroupMascots);
  const groupLogos = useTabGroupLogos();
  const projectKey = projectName(cwd);
  const label = resolveTabGroupLabel(
    projectKey,
    groupLabels,
    basename(cwd) || projectKey,
  );
  const logoPath = resolveTabGroupLogo(projectKey, groupLogos);
  const color = resolveTabGroupColor(
    projectKey,
    groupColors,
    groupCustomColors,
    projectKey,
  );

  return (
    <div
      className="flex h-9 items-center gap-0.5 border-b border-content/10 px-2"
      data-tauri-drag-region="deep"
    >
      <CwdPicker
        cwd={cwd}
        recents={recents}
        placement="below"
        chevron
        onCwdChange={onSelectProject}
        onNewTerminal={onNewTerminal}
        className="min-w-0 items-center"
        buttonClassName="flex h-6.5 w-full items-center gap-1.5 rounded-md px-2 text-[12px] leading-none text-content/50 hover:text-content"
      >
        {logoPath ? (
          <ProjectLogoIcon
            path={logoPath}
            className="size-3.5 shrink-0 rounded-sm"
            imageClassName="size-3.5"
          />
        ) : (
          <ProjectMascot
            project={projectKey}
            color={color}
            name={resolveTabGroupMascot(projectKey, groupMascots)}
            className="size-3 shrink-0"
            active={busy}
          />
        )}
        <span className="min-w-0 truncate">{label}</span>
      </CwdPicker>
      <div className="flex items-center ml-auto">
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
      </div>
    </div>
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
        open || active ? "bg-content/10 text-content" : ""
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
  canReorder = false,
  busy,
  done,
  needsApproval,
  onPointerDown,
  onToggle,
  onContextMenu,
  onRename,
}: {
  folder: SessionFolder;
  sessions: SessionSummary[];
  expanded: boolean;
  dropTarget: boolean;
  canReorder?: boolean;
  busy: boolean;
  done: boolean;
  needsApproval: boolean;
  onPointerDown?: (event: ReactPointerEvent<HTMLButtonElement>) => void;
  onToggle: () => void;
  onContextMenu: (event: ReactMouseEvent<HTMLButtonElement>) => void;
  onRename: () => void;
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
        if (event.key === "F2") {
          event.preventDefault();
          onRename();
        }
      }}
      className={`group relative flex w-full touch-none items-center gap-1.5 px-2 h-8 text-left ${
        expanded ? "rounded-md" : ""
      } ${canReorder ? "cursor-grab active:cursor-grabbing" : ""} ${
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
          <ChevronDown className="size-3.5 text-content" strokeWidth={1.75} />
        ) : (
          <>
            <Folder
              className={`size-3.5 group-hover:hidden group-focus-visible:hidden text-content`}
              strokeWidth={1.75}
            />
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

function SessionCard({
  session,
  isActive,
  busy,
  done,
  needsApproval,
  dropTarget,
  compact = false,
  now,
  onSelect,
  onPrefetch,
  onPlaceOnPane,
  onListDrop,
  onListDropTargetChange,
  onContextMenu,
  onRename,
  onDelete,
}: {
  session: SessionSummary;
  isActive: boolean;
  busy: boolean;
  done: boolean;
  needsApproval: boolean;
  dropTarget?: boolean;
  compact?: boolean;
  now: number;
  onSelect: (sessionId: string) => void;
  onPrefetch?: (sessionId: string) => void;
  onPlaceOnPane?: (sessionId: string, targetId: string, edge: PaneEdge) => void;
  onListDrop?: (draggedId: string, target: SessionListDropTarget) => void;
  onListDropTargetChange?: (target: SessionListDropTarget | null) => void;
  onContextMenu?: (e: ReactMouseEvent<HTMLButtonElement>) => void;
  onRename?: () => void;
  onDelete?: () => void;
}) {
  const skipClickUntil = useRef(0);
  const [dragging, setDragging] = useState(false);
  const title = sessionDisplayTitle(session.title, session.harness);
  const gitLabel = formatGitLabel(session.repo, session.branch);
  const time = formatRelative(session.updatedAt, now);
  const model = compact
    ? null
    : resolveModel(session.harness, session.model).name;
  const statusClass = needsApproval
    ? "text-amber-400"
    : busy
      ? "text-accent"
      : done
        ? "text-emerald-400"
        : "text-content/45";
  const status = (
    <span
      className={`flex shrink-0 items-center gap-1 text-[11px] tabular-nums ${statusClass}`}
    >
      {needsApproval ? (
        <>
          <CircleAlert className="size-3" strokeWidth={1.75} />
          <span>Need approval</span>
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
      ) : (
        <span>{time}</span>
      )}
    </span>
  );

  const onKeyDown = (e: ReactKeyboardEvent<HTMLButtonElement>) => {
    if (e.key === "F2" && onRename) {
      e.preventDefault();
      onRename();
      return;
    }
    if ((e.key === "Delete" || e.key === "Backspace") && onDelete) {
      e.preventDefault();
      onDelete();
    }
  };

  const onPointerDown = (event: ReactPointerEvent<HTMLButtonElement>) => {
    if (event.button !== 0) return;
    // Warm the transcript during the press. Opening stays on click so a
    // drag-to-pane gesture does not switch conversations.
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
        sessionListDropFromPoint(ev.clientX, ev.clientY, session.id),
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
      const listOver = sessionListDropFromPoint(lastX, lastY, session.id);
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

  return (
    <button
      type="button"
      title={title}
      aria-current={isActive ? "true" : undefined}
      data-session-card={session.id}
      data-tauri-drag-region="false"
      onPointerDown={onPointerDown}
      onPointerEnter={() => onPrefetch?.(session.id)}
      onClick={() => {
        if (performance.now() < skipClickUntil.current) return;
        onSelect(session.id);
      }}
      onContextMenu={onContextMenu}
      onKeyDown={onKeyDown}
      className={`relative border flex w-full touch-none flex-col rounded-md px-2.5 text-left ${
        compact ? "py-1.5" : "py-2"
      } ${dragging ? "opacity-40" : ""} ${
        dropTarget
          ? "text-content border-transparent"
          : needsApproval
            ? "bg-content/20 text-content border-content/30 border-dashed"
            : isActive
              ? "bg-content/10 text-content border-transparent"
              : "text-content/80 hover:bg-content/5 hover:text-content border-transparent"
      }`}
    >
      {dropTarget ? (
        <div className="pointer-events-none absolute inset-0 rounded-md bg-accent/20" />
      ) : null}
      {compact ? null : (
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
          {status}
        </span>
      )}
      <span
        className={`relative flex min-w-0 items-center gap-1.5 ${
          compact ? "" : "mt-1"
        }`}
      >
        {session.pinned ? (
          <Pin className="size-3 shrink-0 text-content/45" strokeWidth={1.75} />
        ) : null}
        <span className="min-w-0 flex-1 line-clamp-1 text-[13px] font-semibold leading-snug text-content">
          {title}
        </span>
        {compact ? status : null}
      </span>
      <span className="relative mt-1 flex items-center gap-2">
        {gitLabel ? (
          <span className="flex min-w-0 flex-1 items-center gap-1 text-[11px] text-content/45">
            <GitBranch className="size-3 shrink-0" strokeWidth={1.75} />
            <span className="min-w-0 truncate">{gitLabel}</span>
          </span>
        ) : (
          <span className="min-w-0 flex-1" />
        )}
        <span className="flex shrink-0 items-center gap-1.5">
          <HarnessIcon
            harness={session.harness}
            className="size-3.5 shrink-0"
          />
        </span>
      </span>
    </button>
  );
}

function SessionRenameRow({
  session,
  isActive,
  busy,
  needsApproval,
  onCommit,
  onCancel,
}: {
  session: SessionSummary;
  isActive: boolean;
  busy: boolean;
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
            ? "bg-content/10 text-content"
            : "text-content/80"
      }`}
    >
      <input
        ref={inputRef}
        value={value}
        disabled={busy}
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
  if (additions <= 0 && deletions <= 0) return null;

  const label = [
    additions > 0 ? `+${additions}` : "",
    deletions > 0 ? `-${deletions}` : "",
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <span
      title={`${label} uncommitted`}
      className="flex shrink-0 items-center gap-1.5 font-mono text-[11px] font-semibold tabular-nums"
    >
      {additions > 0 ? (
        <span className="text-emerald-400">+{additions}</span>
      ) : null}
      {deletions > 0 ? (
        <span className="text-red-400">-{deletions}</span>
      ) : null}
    </span>
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
