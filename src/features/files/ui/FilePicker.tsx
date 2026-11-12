import {
  Archive,
  Bot,
  Chatting,
  Folder,
  FolderOpen,
  GitBranch,
  GitCompare,
  Inbox,
  Minus,
  PanelBottom,
  Plus,
  RefreshCw,
  Search,
  Settings,
  StickyNote,
  Terminal,
  Zap,
  type IconComponent,
} from "../../../shared/ui/icons";
import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type MouseEvent as ReactMouseEvent,
} from "react";
import { createPortal } from "react-dom";
import {
  loadProjectFiles,
  peekProjectFiles,
  rankProjectFiles,
  recentOpenedFiles,
  rememberOpenedFile,
  type RankedFile,
} from "../model/fileIndex";
import { LAYER } from "../../../shared/lib/layers";
import { fuzzyMatch, type FuzzyHit } from "../../../shared/lib/fuzzy";
import { projectKey, projectName, prettyCwd } from "../../../shared/lib/paths";
import {
  loadArchivedProjects,
  looksLikeProject,
  subscribeArchivedProjects,
} from "../../projects/model/recents";
import {
  keybindingShortcutLabel,
  subscribeKeybindings,
} from "../../settings/model/settings";
import { loadTabGroupLabels, resolveTabGroupLabel } from "../../workspace/model/tabGroups";
import type { OpenFileFn } from "../../search/model/search";
import { useLockOverscroll } from "../../../shared/hooks/useLockOverscroll";
import { FileTypeIcon } from "./FileTypeIcon";
import { MatchText } from "../../../shared/ui/MatchText";
import { MOD, SHIFT } from "../../../platform/tauri/platform";
type CommandGroup =
  | "Go"
  | "View"
  | "Session"
  | "Project"
  | "App"
  | "Archived conversations"
  | "Archived projects";

type Action = {
  id: string;
  label: string;
  group: CommandGroup;
  hint?: string | null;
  /** Path shown beside an archived project, instead of a shortcut. */
  detail?: string;
  icon: IconComponent;
};

type RankedAction = Action & FuzzyHit;

export type ArchivedConversation = {
  id: string;
  title: string;
  detail?: string;
};

type ArchiveEntry = {
  id: string;
  name: string;
  detail: string;
  group: "Session" | "Project";
  icon: IconComponent;
};

type RankedArchive = ArchiveEntry & {
  score: number;
  namePositions: number[];
  detailPositions: number[];
};

export function reloadActionHint(mod = MOD, shift = SHIFT) {
  return `${mod}${shift}R`;
}

function shortcutHint(command: string, fallback: string) {
  return keybindingShortcutLabel(command, fallback);
}

function commandActions(
  cwd: string,
  notesEnabled: boolean,
  canArchiveConversation: boolean,
): Action[] {
  const actions: Action[] = [
    {
      id: "search",
      label: "Search",
      group: "Go",
      hint: shortcutHint("App: Search", `${MOD}K`),
      icon: Search,
    },
    {
      id: "open-project",
      label: "Open Project",
      group: "Go",
      hint: shortcutHint("App: Open Project", `${MOD}O`),
      icon: FolderOpen,
    },
    {
      id: "settings",
      label: "Settings",
      group: "View",
      hint: shortcutHint("App: Settings", `${MOD},`),
      icon: Settings,
    },
    { id: "inbox", label: "Inbox", group: "View", icon: Inbox },
    { id: "notes", label: "Notes", group: "View", icon: StickyNote },
    { id: "automations", label: "Automations", group: "View", icon: Zap },
    {
      id: "toggle-changes",
      label: "Toggle Changes",
      group: "View",
      icon: GitCompare,
    },
    {
      id: "zoom-in",
      label: "Zoom In",
      group: "View",
      hint: shortcutHint("View: Zoom In", `${MOD}+`),
      icon: Plus,
    },
    {
      id: "zoom-out",
      label: "Zoom Out",
      group: "View",
      hint: shortcutHint("View: Zoom Out", `${MOD}-`),
      icon: Minus,
    },
    {
      id: "new-tab",
      label: "New Tab",
      group: "Session",
      hint: shortcutHint("Tab: New", `${MOD}T`),
      icon: Plus,
    },
    {
      id: "new-terminal",
      label: "New Terminal",
      group: "Session",
      hint: shortcutHint("Terminal: New", `${MOD}\``),
      icon: Terminal,
    },
    {
      id: "toggle-terminal",
      label: "Toggle Terminal",
      group: "Session",
      hint: shortcutHint("Terminal: Toggle Dock", `${MOD}J`),
      icon: PanelBottom,
    },
    {
      id: "switch-model",
      label: "Switch Model",
      group: "Session",
      hint: shortcutHint("App: Switch Model", `${MOD}.`),
      icon: Bot,
    },
    {
      id: "archive-conversation",
      label: "Archive Conversation",
      group: "Session",
      icon: Chatting,
    },
    {
      id: "browse-conversations",
      label: "Archived Conversations",
      group: "Session",
      icon: Archive,
    },
    {
      id: "switch-branch",
      label: "Switch Branch",
      group: "Project",
      icon: GitBranch,
    },
    {
      id: "archive-project",
      label: "Archive Project",
      group: "Project",
      icon: Folder,
    },
    {
      id: "browse-projects",
      label: "Archived Projects",
      group: "Project",
      icon: Archive,
    },
    {
      id: "reload",
      label: "Reload MonoCode",
      group: "App",
      hint: shortcutHint("View: Reload", reloadActionHint()),
      icon: RefreshCw,
    },
  ];
  return actions.filter((action) => {
    if (action.id === "notes" && !notesEnabled) return false;
    if (action.id === "archive-conversation" && !canArchiveConversation) return false;
    if (
      (action.id === "archive-project" || action.id === "switch-branch") &&
      !looksLikeProject(cwd)
    ) {
      return false;
    }
    return true;
  });
}

const RESTORE_ARCHIVE_PREFIX = "restore-archive:";
const RESTORE_CONVERSATION_PREFIX = "restore-conversation:";

function readArchivedProjects(): ArchiveEntry[] {
  const labels = loadTabGroupLabels();
  return loadArchivedProjects().map((project) => ({
    id: `${RESTORE_ARCHIVE_PREFIX}${project.path}`,
    name: resolveTabGroupLabel(
      projectKey(project.path),
      labels,
      projectName(project.path),
    ),
    detail: prettyCwd(project.path),
    group: "Project",
    icon: Folder,
  }));
}

function archiveEntries(
  projects: ArchiveEntry[],
  conversations: ArchivedConversation[],
): ArchiveEntry[] {
  return [
    ...conversations.map((conversation) => ({
      id: `${RESTORE_CONVERSATION_PREFIX}${conversation.id}`,
      name: conversation.title,
      detail: conversation.detail ?? "",
      group: "Session" as const,
      icon: Chatting,
    })),
    ...projects,
  ];
}

function rankArchives(items: ArchiveEntry[], query: string): RankedArchive[] {
  const needle = query.trim();
  if (!needle) {
    return items.map((item) => ({
      ...item,
      score: 0,
      namePositions: [],
      detailPositions: [],
    }));
  }
  const ranked: RankedArchive[] = [];
  for (const item of items) {
    const nameHit = fuzzyMatch(needle, item.name);
    const detailHit = fuzzyMatch(needle, item.detail);
    if (!nameHit && !detailHit) continue;
    ranked.push({
      ...item,
      score: Math.max(nameHit?.score ?? 0, detailHit?.score ?? 0),
      namePositions: nameHit?.positions ?? [],
      detailPositions: detailHit?.positions ?? [],
    });
  }
  ranked.sort(
    (a, b) => b.score - a.score || a.name.localeCompare(b.name),
  );
  return ranked;
}

type Props = {
  open: boolean;
  cwd: string;
  openPaths?: string[];
  initialQuery?: string;
  notesEnabled?: boolean;
  canArchiveConversation?: boolean;
  archivedConversations?: ArchivedConversation[];
  onOpenFile: OpenFileFn;
  onRunAction: (id: string) => void;
  onRestoreArchive?: (path: string) => void;
  onRestoreConversation?: (id: string) => void;
  onClose: () => void;
};

export function FilePicker({
  open,
  cwd,
  openPaths = [],
  initialQuery = "",
  notesEnabled = true,
  canArchiveConversation = true,
  archivedConversations = [],
  onOpenFile,
  onRunAction,
  onRestoreArchive,
  onRestoreConversation,
  onClose,
}: Props) {
  const peekFiles = () => peekProjectFiles(cwd);
  const search = useRef<HTMLInputElement>(null);
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;
  const [query, setQuery] = useState(initialQuery);
  const [commandsRevealed, setCommandsRevealed] = useState(false);
  const [pinnedList, setPinnedList] = useState<
    null | "conversations" | "projects"
  >(null);
  const pinnedListRef = useRef(pinnedList);
  const openedPinnedAt = useRef(0);
  const [projectArchives, setProjectArchives] = useState(readArchivedProjects);
  const archives = useMemo(
    () => archiveEntries(projectArchives, archivedConversations),
    [archivedConversations, projectArchives],
  );
  const [shortcutVersion, setShortcutVersion] = useState(0);
  const [active, setActive] = useState(0);
  const [files, setFiles] = useState(() => peekFiles() ?? []);
  const [loading, setLoading] = useState(() => peekFiles() == null);
  const [error, setError] = useState<string | null>(null);
  const paletteMode = pinnedList != null || query.trim().startsWith(">");
  const actionQuery = query.trim().replace(/^>\s*/, "");
  const actions = useMemo(() => {
    void shortcutVersion;
    return commandActions(cwd, notesEnabled, canArchiveConversation);
  }, [canArchiveConversation, cwd, notesEnabled, shortcutVersion]);

  const recents = useMemo(() => {
    const out: string[] = [];
    const seen = new Set<string>();
    for (const path of [...recentOpenedFiles(cwd), ...openPaths]) {
      if (seen.has(path)) continue;
      seen.add(path);
      out.push(path);
    }
    return out;
  }, [cwd, openPaths, open]);

  const results = useMemo(
    () => (paletteMode ? [] : rankProjectFiles(files, query, recents)),
    [files, paletteMode, query, recents],
  );
  const actionResults = useMemo((): RankedAction[] => {
    if (!paletteMode || !commandsRevealed) return [];
    const ranked = actionQuery
      ? actions.flatMap((action) => {
          const hit = fuzzyMatch(actionQuery, action.label);
          return hit ? [{ ...action, ...hit }] : [];
        })
      : actions.map((action) => ({ ...action, score: 0, positions: [] }));
    if (!actionQuery) return ranked;
    return ranked.sort(
      (a, b) => b.score - a.score || a.label.localeCompare(b.label),
    );
  }, [actionQuery, actions, commandsRevealed, paletteMode]);
  const pinnedResults = useMemo((): RankedAction[] => {
    if (!pinnedList) return [];
    const prefix =
      pinnedList === "conversations"
        ? RESTORE_CONVERSATION_PREFIX
        : RESTORE_ARCHIVE_PREFIX;
    return rankArchives(
      archives.filter((item) => item.id.startsWith(prefix)),
      actionQuery,
    ).map((item) => ({
      id: item.id,
      label: item.name,
      group:
        pinnedList === "conversations"
          ? ("Archived conversations" as const)
          : ("Archived projects" as const),
      detail: item.detail,
      icon: item.icon,
      score: item.score,
      positions: item.namePositions,
    }));
  }, [actionQuery, archives, pinnedList]);
  const shownActions = pinnedList ? pinnedResults : actionResults;
  const pinnedResultsRef = useRef(pinnedResults);
  const actionResultsRef = useRef(actionResults);
  const activeIndexRef = useRef(active);
  pinnedListRef.current = pinnedList;
  pinnedResultsRef.current = pinnedResults;
  actionResultsRef.current = actionResults;
  activeIndexRef.current = active;
  const optionCount = paletteMode ? shownActions.length : results.length;

  useEffect(
    () => subscribeKeybindings(() => setShortcutVersion((version) => version + 1)),
    [],
  );
  useEffect(
    () =>
      subscribeArchivedProjects(() => setProjectArchives(readArchivedProjects())),
    [],
  );

  useEffect(() => {
    if (!open) return;
    pinnedListRef.current = null;
    openedPinnedAt.current = 0;
    setPinnedList(null);
    setCommandsRevealed(false);
    setQuery(initialQuery);
    setActive(0);
    setError(null);
    const searchable = looksLikeProject(cwd);
    const cached = peekFiles();
    if (cached) {
      setFiles(cached);
      setLoading(false);
    } else {
      setFiles([]);
      setLoading(searchable);
    }
    if (!searchable) {
      setLoading(false);
      return;
    }
    let cancelled = false;
    void loadProjectFiles(cwd, true)
      .then((next) => {
        if (cancelled) return;
        setFiles(next);
        setLoading(false);
      })
      .catch((err: unknown) => {
        if (cancelled) return;
        setError(err instanceof Error ? err.message : String(err));
        setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [open, cwd, initialQuery]);

  useEffect(() => {
    setActive((index) =>
      optionCount === 0 ? 0 : Math.min(index, optionCount - 1),
    );
  }, [optionCount]);

  useEffect(() => {
    if (!open) return;
    search.current?.focus();
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.preventDefault();
      e.stopPropagation();
      onCloseRef.current();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [open]);

  if (!open) return null;

  const pick = (file: RankedFile) => {
    rememberOpenedFile(cwd, file.path);
    onOpenFile(file.path, undefined, { exact: true });
    onClose();
  };
  const showPinned = (next: "conversations" | "projects") => {
    pinnedListRef.current = next;
    openedPinnedAt.current = performance.now();
    setProjectArchives(readArchivedProjects());
    setPinnedList(next);
    setCommandsRevealed(true);
    setQuery(">");
    setActive(0);
  };
  const runAction = (action: RankedAction) => {
    if (
      action.id === "browse-conversations" ||
      action.id === "browse-projects"
    ) {
      showPinned(
        action.id === "browse-conversations" ? "conversations" : "projects",
      );
      return;
    }
    // The archive list replaces commands. A second Enter in the same turn
    // still sees the command row (Search is first) until React re-renders.
    if (
      pinnedListRef.current &&
      !action.id.startsWith(RESTORE_ARCHIVE_PREFIX) &&
      !action.id.startsWith(RESTORE_CONVERSATION_PREFIX)
    ) {
      return;
    }
    if (openArchiveEntry(action.id)) return;
    onRunAction(action.id);
    onClose();
  };
  const openArchiveEntry = (id: string) => {
    if (id.startsWith(RESTORE_ARCHIVE_PREFIX)) {
      onRestoreArchive?.(id.slice(RESTORE_ARCHIVE_PREFIX.length));
      onClose();
      return true;
    }
    if (id.startsWith(RESTORE_CONVERSATION_PREFIX)) {
      onRestoreConversation?.(id.slice(RESTORE_CONVERSATION_PREFIX.length));
      onClose();
      return true;
    }
    return false;
  };
  const onSearchKey = (e: ReactKeyboardEvent<HTMLInputElement>) => {
    if (
      pinnedListRef.current &&
      e.key === "Backspace" &&
      query.trim() === ">"
    ) {
      e.preventDefault();
      pinnedListRef.current = null;
      openedPinnedAt.current = 0;
      setPinnedList(null);
      setActive(0);
      return;
    }
    if (e.key === "ArrowDown") {
      e.preventDefault();
      if (optionCount === 0) return;
      setActive((index) => (index + 1) % optionCount);
      return;
    }
    if (e.key === "ArrowUp") {
      e.preventDefault();
      if (optionCount === 0) return;
      setActive((index) => (index - 1 + optionCount) % optionCount);
      return;
    }
    if (e.key === "Enter") {
      e.preventDefault();
      e.stopPropagation();
      if (paletteMode) {
        if (pinnedListRef.current) {
          if (performance.now() - openedPinnedAt.current < 400) return;
          const action = pinnedResultsRef.current[activeIndexRef.current];
          if (action) runAction(action);
          return;
        }
        const action = actionResultsRef.current[activeIndexRef.current];
        if (action) runAction(action);
      } else {
        const file = results[active];
        if (file) pick(file);
      }
      return;
    }
    if (e.key === "Tab") e.preventDefault();
  };

  const empty = emptyLabel({
    cwd,
    query,
    loading,
    error,
    fileCount: files.length,
    matchCount: results.length,
    paletteMode,
    pinnedList,
    commandsRevealed,
    actionCount: shownActions.length,
    archiveCount: pinnedResults.length,
  });

  return createPortal(
    <div className="fixed inset-0" style={{ zIndex: LAYER.dialog }}>
      <div className="absolute inset-0" onMouseDown={onClose} />
      <div
        role="dialog"
        aria-label={paletteMode ? "Command Palette" : "Go to File"}
        data-file-picker
        onMouseDown={(e) => e.stopPropagation()}
        className="absolute left-1/2 top-[12%] flex w-[min(560px,calc(100vw-24px))] -translate-x-1/2 flex-col overflow-hidden rounded-lg border border-content/10 bg-content/5 backdrop-blur-xl"
      >
        <div className="pb-1.5">
          <label className="flex items-center gap-2 border-b border-stroke px-2 py-2.5 text-content/50">
            <Search className="size-3.5 shrink-0" strokeWidth={1.75} />
            <input
              ref={search}
              type="text"
              value={query}
              placeholder="Go to File (type > for commands)"
              aria-label={paletteMode ? "Command Palette" : "Go to File"}
              spellCheck={false}
              autoComplete="off"
              autoCorrect="off"
              autoCapitalize="off"
              className="min-w-0 flex-1 bg-transparent text-[13px] text-content outline-none placeholder:text-content/40"
              onChange={(e) => {
                const next = e.target.value;
                openedPinnedAt.current = 0;
                setQuery(next);
                setActive(0);
                if (next.trim().startsWith(">")) setCommandsRevealed(true);
                else {
                  pinnedListRef.current = null;
                  setPinnedList(null);
                }
              }}
              onKeyDown={onSearchKey}
            />
          </label>
        </div>
        {empty ? (
          <p className="px-3 pb-3 pt-1 text-[12px] text-content/50">{empty}</p>
        ) : paletteMode ? (
          <ActionList
            actions={shownActions}
            active={active}
            query={actionQuery}
            onActive={setActive}
            onRun={runAction}
          />
        ) : (
          <FileList
            files={results}
            active={active}
            query={query}
            onActive={setActive}
            onPick={pick}
          />
        )}
      </div>
    </div>,
    document.body,
  );
}

function emptyLabel({
  cwd,
  query,
  loading,
  error,
  fileCount,
  matchCount,
  paletteMode,
  pinnedList,
  commandsRevealed,
  actionCount,
  archiveCount,
}: {
  cwd: string;
  query: string;
  loading: boolean;
  error: string | null;
  fileCount: number;
  matchCount: number;
  paletteMode: boolean;
  pinnedList: null | "conversations" | "projects";
  commandsRevealed: boolean;
  actionCount: number;
  archiveCount: number;
}): string | null {
  if (pinnedList) {
    if (archiveCount === 0) {
      const noun = pinnedList === "conversations" ? "conversations" : "projects";
      const filtering = query.trim().replace(/^>\s*/, "");
      return filtering ? `No matching ${noun}` : `No archived ${noun}`;
    }
    return null;
  }
  if (paletteMode) {
    if (!commandsRevealed) return "Type a command";
    return actionCount === 0 ? "No matching commands" : null;
  }
  if (error && fileCount === 0) return error;
  if (!looksLikeProject(cwd)) return "Open a project to search files";
  if (loading && fileCount === 0) return "Indexing files…";
  if (fileCount === 0) return "No files found";
  if (matchCount === 0) {
    return query.trim() ? "No matching files" : "Type a file name to search";
  }
  return null;
}

function ActionList({
  actions,
  active,
  query,
  onActive,
  onRun,
}: {
  actions: RankedAction[];
  active: number;
  query: string;
  onActive: (index: number) => void;
  onRun: (action: RankedAction) => void;
}) {
  const lockOverscroll = useLockOverscroll<HTMLDivElement>();
  const activeRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    activeRef.current?.scrollIntoView({ block: "nearest" });
  }, [active]);

  // Keep score order. Pulling a group back together renumbers the rows, so
  // Enter would run a different command than the highlighted one.
  return (
    <div
      ref={lockOverscroll}
      role="listbox"
      aria-label="Commands"
      className="max-h-[min(380px,50vh)] overflow-y-auto overscroll-none px-1.5 pb-2"
    >
      {actions.map((action, index) => {
        const showHeader =
          index === 0 || actions[index - 1].group !== action.group;
        const endOfGroup =
          index === actions.length - 1 ||
          actions[index + 1].group !== action.group;
        const highlighted = index === active;
        return (
          <div key={action.id} className={endOfGroup ? "pb-1" : undefined}>
            {showHeader ? (
              <div className="px-2 pb-1 pt-2 text-[10px] font-medium uppercase tracking-wide text-content/40">
                {action.group}
              </div>
            ) : null}
            <button
                ref={highlighted ? activeRef : undefined}
                type="button"
                role="option"
                aria-selected={highlighted}
                onMouseDown={(e) => e.preventDefault()}
                onMouseEnter={() => onActive(index)}
                onClick={() => onRun(action)}
                className={`flex min-h-9 w-full items-center gap-2.5 rounded-md px-2 text-left text-[13px] leading-5 ${
                  highlighted ? "bg-selection text-content" : "text-content"
                }`}
              >
                <ActionIcon icon={action.icon} />
                <span className="min-w-0 flex-1 truncate leading-5">
                  <MatchText
                    text={action.label}
                    positions={action.positions}
                    active={Boolean(query)}
                  />
                </span>
                {action.detail ? (
                  <span className="min-w-0 max-w-[45%] truncate text-right font-mono text-[11px] leading-5 text-content/40">
                    {action.detail}
                  </span>
                ) : (
                  <span className="w-28 shrink-0 text-right">
                    {action.hint ? (
                      <kbd className="rounded border border-content/10 bg-content/5 px-1.5 py-0.5 font-mono text-[10px] leading-4 text-content/50">
                        {action.hint}
                      </kbd>
                    ) : null}
                  </span>
                )}
            </button>
          </div>
        );
      })}
    </div>
  );
}
function ActionIcon({ icon: Icon }: { icon: IconComponent }) {
  return (
    <Icon className="size-4 shrink-0 text-content/50" strokeWidth={1.75} />
  );
}

function FileList({
  files,
  active,
  query,
  onActive,
  onPick,
}: {
  files: RankedFile[];
  active: number;
  query: string;
  onActive: (index: number) => void;
  onPick: (file: RankedFile) => void;
}) {
  const lockOverscroll = useLockOverscroll<HTMLDivElement>();
  const activeRef = useRef<HTMLButtonElement>(null);
  const pointer = useRef({ x: Number.NaN, y: Number.NaN, allow: false });
  const fromPointer = useRef(false);

  useEffect(() => {
    pointer.current.allow = false;
  }, [files]);

  useEffect(() => {
    if (fromPointer.current) {
      fromPointer.current = false;
      return;
    }
    pointer.current.allow = false;
    activeRef.current?.scrollIntoView({ block: "nearest" });
  }, [active]);

  const onListMouseMove = (e: ReactMouseEvent<HTMLDivElement>) => {
    if (e.clientX === pointer.current.x && e.clientY === pointer.current.y) {
      return;
    }
    pointer.current = { x: e.clientX, y: e.clientY, allow: true };
  };

  const onRowEnter = (index: number) => {
    if (!pointer.current.allow) return;
    fromPointer.current = true;
    onActive(index);
  };

  return (
    <div
      ref={lockOverscroll}
      role="listbox"
      aria-label="Files"
      onMouseMove={onListMouseMove}
      className="max-h-[min(380px,50vh)] overflow-y-auto overscroll-none px-1.5 pb-1.5"
    >
      {files.map((file, index) => {
        const highlighted = index === active;
        const slash = file.relative.lastIndexOf("/");
        const dir = slash === -1 ? "" : file.relative.slice(0, slash);
        const nameOffset = slash === -1 ? 0 : slash + 1;
        return (
          <button
            key={file.path}
            ref={highlighted ? activeRef : undefined}
            type="button"
            role="option"
            aria-selected={highlighted}
            onMouseDown={(e) => e.preventDefault()}
            onMouseEnter={() => onRowEnter(index)}
            onClick={() => onPick(file)}
            className={`flex h-8 w-full items-center gap-2 rounded-md px-2 text-left text-sm leading-none ${
              highlighted ? "bg-selection text-content" : "text-content"
            }`}
          >
            <span className="shrink-0">
              <FileTypeIcon name={file.name} isDir={false} />
            </span>
            <span className="min-w-0 flex-1 truncate">
              <MatchText
                text={file.name}
                positions={file.positions
                  .filter((pos) => pos >= nameOffset)
                  .map((pos) => pos - nameOffset)}
                active={Boolean(query.trim())}
              />
            </span>
            {dir ? (
              <span className="min-w-0 max-w-[45%] truncate font-mono text-[11px] text-content/40">
                <MatchText
                  text={dir}
                  positions={file.positions.filter((pos) => pos < slash)}
                  active={Boolean(query.trim())}
                />
              </span>
            ) : null}
          </button>
        );
      })}
    </div>
  );
}
