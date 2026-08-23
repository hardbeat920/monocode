import { CircleAlert, GitBranch, GitCompare } from "lucide-react";
import {
  memo,
  useEffect,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type MouseEvent as ReactMouseEvent,
  type PointerEvent,
} from "react";
import {
  loadSidebarTabOrder,
  saveSidebarTabOrder,
  type SidebarTabId,
} from "../lib/appearance";
import { basename } from "../lib/fs";
import { resolveModel } from "../lib/models";
import { projectName } from "../lib/paths";
import { useTabGroupLogos } from "../lib/projectLogos";
import { sessionDisplayTitle } from "../lib/session";
import type { SessionSummary } from "../lib/sessionStore";
import { resolveTabGroupLogo } from "../lib/tabGroups";
import { useLockOverscroll } from "../lib/useLockOverscroll";
import { useProjectDiffStats } from "../lib/useProjectDiffStats";
import { useSessionDiffStats } from "../lib/useSessionDiffStats";
import { useSortable } from "../lib/useSortable";
import { ExplorerMenu, type ExplorerMenuItem } from "./ExplorerMenu";
import { FileTree } from "./FileTree";
import { FileTypeIcon } from "./FileTypeIcon";
import { HarnessIcon } from "./HarnessIcon";
import { TerminalSpinner } from "./TerminalSpinner";
import { TabVisitNav } from "./TitleBar";
import { ProjectSearch } from "./ProjectSearch";
import { ProjectLogoIcon } from "./ProjectLogoIcon";
import { SidebarUpdate } from "./SidebarUpdate";

const MIN_WIDTH = 160;
const MAX_WIDTH = 560;
const DEFAULT_WIDTH = 260;

let rememberedWidth = DEFAULT_WIDTH;

type SidebarTab = SidebarTabId;

const TAB_LABELS: Record<SidebarTab, string> = {
  files: "Files",
  sessions: "Sessions",
};

type Props = {
  cwd: string;
  open: boolean;
  sessions: SessionSummary[];
  busySessionIds: Set<string>;
  approvalSessionIds: Set<string>;
  activeSessionId?: string;
  status: "idle" | "loading" | "error";
  onSelectSession: (sessionId: string) => void;
  onRenameSession?: (sessionId: string, title: string) => void;
  onDeleteSession?: (sessionId: string) => void;
  onOpenFile: (path: string) => void;
  onOpenTerminal?: (cwd: string) => void;
  onFileMoved?: (from: string, to: string) => void;
  onFileDeleted?: (path: string) => void;
  diffOpen?: boolean;
  onToggleDiff?: () => void;
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
};

function SidebarComponent({
  cwd,
  open,
  sessions,
  busySessionIds,
  approvalSessionIds,
  activeSessionId,
  status,
  onSelectSession,
  onRenameSession,
  onDeleteSession,
  onOpenFile,
  onOpenTerminal,
  onFileMoved,
  onFileDeleted,
  diffOpen = false,
  onToggleDiff,
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
}: Props) {
  const [width, setWidth] = useState(rememberedWidth);
  const [dragging, setDragging] = useState(false);
  const [tabOrder, setTabOrder] = useState<SidebarTab[]>(loadSidebarTabOrder);
  const [now, setNow] = useState(() => Date.now());
  const drag = useRef<{ startX: number; startW: number } | null>(null);
  const asideRef = useRef<HTMLElement>(null);
  const widthRef = useRef(width);
  const pendingWidth = useRef(width);
  const resizeFrame = useRef<number | null>(null);
  const lockOverscroll = useLockOverscroll<HTMLDivElement>();
  const sessionsLock = useLockOverscroll<HTMLDivElement>();
  const sessionsScrollRef = useRef<HTMLDivElement>(null);
  const [sessionMenu, setSessionMenu] = useState<{
    x: number;
    y: number;
    sessionId: string;
  } | null>(null);
  const [renamingSessionId, setRenamingSessionId] = useState<string | null>(
    null,
  );
  const sortable = useSortable(tabOrder, (ids) => {
    const next = ids as SidebarTab[];
    setTabOrder(next);
    saveSidebarTabOrder(next);
    if (next[0]) onTabChange(next[0]);
  });
  const canDragTabs = tabOrder.length > 1;
  const projectDiff = useProjectDiffStats(cwd, open);
  const groupLogos = useTabGroupLogos();
  const projectLogoPath = resolveTabGroupLogo(projectName(cwd), groupLogos);
  const sessionDiffs = useSessionDiffStats(
    cwd,
    sessions.map((session) => session.id),
    open && tab === "sessions",
  );

  useEffect(() => {
    if (tab !== "sessions") return;
    const id = window.setInterval(() => setNow(Date.now()), 30_000);
    return () => window.clearInterval(id);
  }, [tab]);

  useEffect(() => {
    if (!sessionMenu) return;
    const onScroll = () => setSessionMenu(null);
    const scrollParent = sessionsScrollRef.current ?? window;
    scrollParent.addEventListener("scroll", onScroll, true);
    return () => scrollParent.removeEventListener("scroll", onScroll, true);
  }, [sessionMenu]);

  const sessionMenuItems: ExplorerMenuItem[] = [
    { kind: "item", id: "rename", label: "Rename", shortcut: "F2" },
    { kind: "sep" },
    {
      kind: "item",
      id: "delete",
      label: "Delete",
      shortcut: "⌫",
      danger: true,
    },
  ];

  const onSessionContextMenu = (
    sessionId: string,
    e: ReactMouseEvent<HTMLButtonElement>,
  ) => {
    if (!onRenameSession && !onDeleteSession) return;
    e.preventDefault();
    e.stopPropagation();
    setSessionMenu({ x: e.clientX, y: e.clientY, sessionId });
  };

  const onSessionMenuPick = (id: string) => {
    if (!sessionMenu) return;
    const sessionId = sessionMenu.sessionId;
    setSessionMenu(null);
    if (id === "rename") {
      setRenamingSessionId(sessionId);
      return;
    }
    if (id === "delete") onDeleteSession?.(sessionId);
  };

  const clamp = (value: number) => {
    const max = Math.min(MAX_WIDTH, Math.floor(window.innerWidth * 0.5));
    return Math.min(max, Math.max(MIN_WIDTH, Math.round(value)));
  };

  useEffect(() => {
    if (!dragging) return;
    const previous = document.body.style.cursor;
    document.body.style.cursor = "col-resize";
    return () => {
      document.body.style.cursor = previous;
    };
  }, [dragging]);

  useEffect(
    () => () => {
      if (resizeFrame.current != null) {
        cancelAnimationFrame(resizeFrame.current);
      }
    },
    [],
  );

  const paintWidth = (next: number) => {
    pendingWidth.current = next;
    if (resizeFrame.current != null) return;
    resizeFrame.current = requestAnimationFrame(() => {
      resizeFrame.current = null;
      rememberedWidth = pendingWidth.current;
      if (asideRef.current) {
        asideRef.current.style.width = `${pendingWidth.current}px`;
      }
    });
  };

  const commitWidth = () => {
    if (resizeFrame.current != null) {
      cancelAnimationFrame(resizeFrame.current);
      resizeFrame.current = null;
    }
    const next = pendingWidth.current;
    rememberedWidth = next;
    widthRef.current = next;
    if (asideRef.current) asideRef.current.style.width = `${next}px`;
    setWidth(next);
  };

  const onResizePointerDown = (e: PointerEvent<HTMLDivElement>) => {
    e.preventDefault();
    e.currentTarget.setPointerCapture(e.pointerId);
    drag.current = { startX: e.clientX, startW: widthRef.current };
    pendingWidth.current = widthRef.current;
    setDragging(true);
  };

  const onResizePointerMove = (e: PointerEvent<HTMLDivElement>) => {
    if (!drag.current) return;
    const next = clamp(drag.current.startW + (e.clientX - drag.current.startX));
    paintWidth(next);
  };

  const onResizePointerUp = (e: PointerEvent<HTMLDivElement>) => {
    if (!drag.current) return;
    drag.current = null;
    commitWidth();
    setDragging(false);
    if (e.currentTarget.hasPointerCapture(e.pointerId)) {
      e.currentTarget.releasePointerCapture(e.pointerId);
    }
  };

  const onResizeDoubleClick = () => {
    pendingWidth.current = DEFAULT_WIDTH;
    commitWidth();
  };

  return (
    <aside
      ref={asideRef}
      style={{ width }}
      className={`sidebar-glass relative shrink-0 flex-col border-r border-content/10 ${
        open ? "flex" : "hidden"
      }`}
    >
      <div
        className="flex h-9.75 shrink-0 items-center justify-end pr-1.5"
        data-tauri-drag-region
      >
        <TabVisitNav
          canGoBack={canGoBack}
          canGoForward={canGoForward}
          onGoBack={onGoBack}
          onGoForward={onGoForward}
        />
      </div>
      <div
        role="tablist"
        aria-label="Sidebar"
        className="flex shrink-0 border-y border-content/10"
      >
        {tabOrder.map((itemId, index) => {
          const active = tab === itemId;
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
                onTabChange(itemId);
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
                onClick={() => {
                  if (sortable.consumeClick()) return;
                  onTabChange(itemId);
                }}
                className={`relative h-9 min-w-0 flex-1 text-[12px] leading-none ${
                  canDragTabs ? "cursor-grab active:cursor-grabbing" : ""
                } ${
                  active
                    ? "text-content bg-content/10"
                    : "text-content/50 hover:text-content"
                }`}
              >
                {TAB_LABELS[itemId]}
                {active ? (
                  <span className="absolute inset-x-0 bottom-0 h-px bg-content" />
                ) : null}
              </button>
            </div>
          );
        })}
      </div>
      <div
        className={`flex min-h-0 flex-1 flex-col overflow-hidden ${
          tab === "files" ? "" : "hidden"
        }`}
      >
        {filesSearchOpen ? (
          <ProjectSearch
            cwd={cwd}
            focusToken={searchFocusToken}
            onOpenFile={onOpenFile}
            onClose={() => onFilesSearchOpenChange(false)}
          />
        ) : cwd && cwd !== "~" ? (
          <div
            ref={lockOverscroll}
            className="min-h-0 flex-1 overflow-y-auto overscroll-none"
          >
            <FileTree
              key={cwd}
              cwd={cwd}
              onOpenFile={onOpenFile}
              onOpenTerminal={onOpenTerminal}
              onFileMoved={onFileMoved}
              onFileDeleted={onFileDeleted}
              onSearch={onOpenFilesSearch}
              headerEnd={
                onToggleDiff ? (
                  <DiffStat
                    files={projectDiff?.files ?? 0}
                    additions={projectDiff?.additions ?? 0}
                    deletions={projectDiff?.deletions ?? 0}
                    active={diffOpen}
                    onClick={onToggleDiff}
                    variant="icon"
                  />
                ) : null
              }
            />
          </div>
        ) : (
          <p className="px-3 py-2 text-[12px] text-content/50">
            No project folder
          </p>
        )}
      </div>
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
            <div className="sticky top-0 flex h-8 items-center bg-content/5 backdrop-blur-md">
              <div
                title={cwd}
                className="flex h-full min-w-0 flex-1 items-center gap-1.5 px-2"
              >
                {projectLogoPath ? (
                  <ProjectLogoIcon
                    path={projectLogoPath}
                    className="size-4 shrink-0 rounded-sm ml-1.5"
                    imageClassName="size-4"
                  />
                ) : (
                  <span className="grid size-6 shrink-0 place-items-center">
                    <FileTypeIcon name={basename(cwd)} isDir isRoot />
                  </span>
                )}
                <span className="min-w-0 flex-1 truncate text-[11px] font-semibold tracking-[0.08em] text-content/50 uppercase">
                  {basename(cwd)}
                </span>
                <DiffStat
                  files={projectDiff?.files ?? 0}
                  additions={projectDiff?.additions ?? 0}
                  deletions={projectDiff?.deletions ?? 0}
                  active={diffOpen}
                  onClick={onToggleDiff}
                />
              </div>
            </div>
            {status === "loading" && sessions.length === 0 ? (
              <p className="px-3 py-2 text-[12px] text-content/50">Loading…</p>
            ) : status === "error" && sessions.length === 0 ? (
              <p className="px-3 py-2 text-[12px] text-content/50">
                Couldn’t load sessions
              </p>
            ) : sessions.length === 0 ? (
              <p className="px-3 py-2 text-[12px] text-content/50">
                No sessions yet
              </p>
            ) : (
              <ul className="flex flex-col gap-0.5 p-1.5">
                {sessions.map((session) => (
                  <li key={session.id}>
                    {renamingSessionId === session.id && onRenameSession ? (
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
                        needsApproval={approvalSessionIds.has(session.id)}
                        now={now}
                        additions={sessionDiffs[session.id]?.additions ?? 0}
                        deletions={sessionDiffs[session.id]?.deletions ?? 0}
                        onSelect={onSelectSession}
                        onContextMenu={
                          onRenameSession || onDeleteSession
                            ? (e) => onSessionContextMenu(session.id, e)
                            : undefined
                        }
                        onRename={
                          onRenameSession
                            ? () => setRenamingSessionId(session.id)
                            : undefined
                        }
                        onDelete={
                          onDeleteSession
                            ? () => onDeleteSession(session.id)
                            : undefined
                        }
                      />
                    )}
                  </li>
                ))}
              </ul>
            )}
          </div>
        )}
      </div>
      <SidebarUpdate />
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
      <div
        role="separator"
        aria-orientation="vertical"
        aria-label="Resize sidebar"
        aria-valuenow={width}
        aria-valuemin={MIN_WIDTH}
        aria-valuemax={MAX_WIDTH}
        className={`absolute inset-y-0 -right-px z-10 w-1.5 cursor-col-resize touch-none ${
          dragging ? "bg-content/15" : "hover:bg-content/10"
        }`}
        onPointerDown={onResizePointerDown}
        onPointerMove={onResizePointerMove}
        onPointerUp={onResizePointerUp}
        onPointerCancel={onResizePointerUp}
        onDoubleClick={onResizeDoubleClick}
      />
    </aside>
  );
}

export const Sidebar = memo(SidebarComponent);

function SessionCard({
  session,
  isActive,
  busy,
  needsApproval,
  now,
  additions,
  deletions,
  onSelect,
  onContextMenu,
  onRename,
  onDelete,
}: {
  session: SessionSummary;
  isActive: boolean;
  busy: boolean;
  needsApproval: boolean;
  now: number;
  additions: number;
  deletions: number;
  onSelect: (sessionId: string) => void;
  onContextMenu?: (e: ReactMouseEvent<HTMLButtonElement>) => void;
  onRename?: () => void;
  onDelete?: () => void;
}) {
  const title = sessionDisplayTitle(session.title, session.harness);
  const gitLabel = formatGitLabel(session.repo, session.branch);
  const time = formatRelative(session.updatedAt, now);
  const model = resolveModel(session.harness, session.model).name;

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

  return (
    <button
      type="button"
      title={title}
      aria-current={isActive ? "true" : undefined}
      onClick={() => onSelect(session.id)}
      onContextMenu={onContextMenu}
      onKeyDown={onKeyDown}
      className={`border flex w-full flex-col rounded-md px-2.5 py-2 text-left ${
        needsApproval
          ? "bg-content/20 text-content border-content/30 border-dashed"
          : isActive
            ? "bg-content/10 text-content border-transparent"
            : "text-content/80 hover:bg-content/5 hover:text-content border-transparent"
      }`}
    >
      <span className="flex items-center gap-2">
        <span className="flex min-w-0 flex-1 items-center gap-1.5">
          <HarnessIcon
            harness={session.harness}
            className="size-3.5 shrink-0"
          />
          <span className="min-w-0 truncate text-[11px] text-content/50">
            {model}
          </span>
        </span>
        <span
          className={`flex shrink-0 items-center gap-1 text-[11px] tabular-nums ${
            needsApproval
              ? "text-amber-400"
              : busy
                ? "text-accent"
                : "text-content/45"
          }`}
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
          ) : (
            <span>{time}</span>
          )}
        </span>
      </span>
      <span className="mt-1 line-clamp-1 text-[13px] font-semibold leading-snug text-content">
        {title}
      </span>
      <span className="mt-1 flex items-center gap-2">
        {gitLabel ? (
          <span className="flex min-w-0 flex-1 items-center gap-1 text-[11px] text-content/45">
            <GitBranch className="size-3 shrink-0" strokeWidth={1.75} />
            <span className="min-w-0 truncate">{gitLabel}</span>
          </span>
        ) : (
          <span className="min-w-0 flex-1" />
        )}
        <span className="flex shrink-0 items-center gap-1.5">
          <DiffStat additions={additions} deletions={deletions} />
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
  files,
  additions,
  deletions,
  active,
  onClick,
  variant = "chip",
}: {
  files?: number;
  additions: number;
  deletions: number;
  active?: boolean;
  onClick?: () => void;
  variant?: "chip" | "icon";
}) {
  const fileCount = files ?? 0;
  const empty = fileCount <= 0 && additions <= 0 && deletions <= 0;
  if (empty && !onClick) return null;

  if (variant === "icon" && onClick) {
    const label = empty
      ? active
        ? "Hide changes"
        : "Show changes"
      : [
          `${fileCount} ${fileCount === 1 ? "file" : "files"} changed`,
          additions > 0 ? `+${additions}` : "",
          deletions > 0 ? `-${deletions}` : "",
        ]
          .filter(Boolean)
          .join(" ");
    const badge = fileCount > 99 ? "99+" : String(fileCount);
    return (
      <button
        type="button"
        title={label}
        aria-label={label}
        aria-pressed={active}
        onMouseDown={(event) => event.preventDefault()}
        onClick={(event) => {
          event.stopPropagation();
          onClick();
        }}
        className={`relative flex-1 h-8 shrink-0 place-items-center text-content/50 hover:bg-content/10 hover:text-content flex items-center justify-center ${
          active ? "bg-content/10 text-content" : ""
        }`}
      >
        <GitCompare className="size-3.5" strokeWidth={1.75} />
        {fileCount > 0 ? (
          <span className="absolute top-3.5 left-7 grid min-h-3.5 min-w-3.5 place-items-center rounded-full bg-accent px-0.5 text-[7px] font-semibold leading-none text-white tabular-nums">
            {badge}
          </span>
        ) : null}
      </button>
    );
  }

  if (empty && onClick) {
    return (
      <button
        type="button"
        title={active ? "Hide changes" : "Show changes"}
        aria-label={active ? "Hide changes" : "Show changes"}
        aria-pressed={active}
        onClick={(event) => {
          event.stopPropagation();
          onClick();
        }}
        className={`grid size-8 shrink-0 place-items-center text-content/50 hover:bg-content/10 hover:text-content ${
          active ? "bg-content/10 text-content" : ""
        }`}
      >
        <GitCompare className="size-3.5" strokeWidth={1.75} />
      </button>
    );
  }

  const label = [
    fileCount > 0 ? `${fileCount} ${fileCount === 1 ? "File" : "Files"}` : "",
    additions > 0 ? `+${additions}` : "",
    deletions > 0 ? `-${deletions}` : "",
  ]
    .filter(Boolean)
    .join(" ");
  const body = (
    <>
      {fileCount > 0 ? (
        <>
          <span className="text-content/70 font-sans font-normal">
            {fileCount} {fileCount === 1 ? "File" : "Files"}
          </span>
          <span className="text-content/70 font-sans font-normal">•</span>
        </>
      ) : null}
      {additions > 0 ? (
        <span className="text-emerald-400">+{additions}</span>
      ) : null}
      {deletions > 0 ? (
        <span className="text-red-400">-{deletions}</span>
      ) : null}
    </>
  );
  const className = `flex shrink-0 items-center gap-1.5 font-mono text-[11px] font-semibold tabular-nums ${
    onClick
      ? `rounded cursor-pointer px-1.5 py-0.5 hover:bg-content/10 ${
          active ? "bg-content/10" : ""
        }`
      : ""
  }`;
  if (onClick) {
    return (
      <button
        type="button"
        title={`${label} uncommitted — review diffs`}
        aria-pressed={active}
        onClick={(event) => {
          event.stopPropagation();
          onClick();
        }}
        className={className}
      >
        {body}
      </button>
    );
  }
  return (
    <span title={`${label} uncommitted`} className={className}>
      {body}
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
