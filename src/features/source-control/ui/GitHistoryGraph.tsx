import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type PointerEvent as ReactPointerEvent,
} from "react";
import {
  ChevronDown,
  ChevronRight,
  GitBranch,
  RefreshCw,
  Search,
  X,
} from "../../../shared/ui/icons";
import { useLockOverscroll } from "../../../shared/hooks/useLockOverscroll";
import { suppressTextSelection } from "../../../shared/lib/drag";
import { PaneHeader } from "../../../shared/ui/PaneHeader";
import {
  gitHistory,
  subscribeGitChanged,
  type GitHistoryCommit,
} from "../../../platform/tauri/fs";
import {
  GRAPH_ROW_PX,
  SWIMLANE_WIDTH,
  historyItemGraph,
  layoutGitGraph,
  type GraphRef,
  type HistoryItemViewModel,
} from "../model/gitGraph";

type Props = {
  cwd: string;
  enabled: boolean;
  expanded?: boolean;
  selectedSha?: string;
  /** Omit for the full-height History tab with its own toolbar. */
  onToggleExpanded?: () => void;
  onOpenCommit: (commit: GitHistoryCommit, pin?: boolean) => void;
};

const historyByCwd = new Map<string, GitHistoryCommit[]>();
const HISTORY_ROW_PX = 44;
const shortDate = new Intl.DateTimeFormat(undefined, {
  month: "short",
  day: "numeric",
});

function softHistoryColor(color: string): string {
  return `color-mix(in oklab, ${color} var(--git-history-color-strength, 70%), var(--color-content))`;
}

export function GitHistoryGraph({
  cwd,
  enabled,
  expanded = true,
  selectedSha,
  onToggleExpanded,
  onOpenCommit,
}: Props) {
  const lockOverscroll = useLockOverscroll<HTMLDivElement>();
  const standalone = !onToggleExpanded;
  const { commits, loading, error, refresh } = useGitHistory(
    cwd,
    enabled && expanded,
  );
  const rows = useMemo(() => layoutGitGraph(commits), [commits]);
  const [search, setSearch] = useState({ cwd, query: "" });
  useEffect(() => {
    setSearch((current) =>
      current.cwd === cwd ? current : { cwd, query: "" },
    );
  }, [cwd]);
  const query = standalone && search.cwd === cwd ? search.query : "";
  const searchText = query.trim().toLocaleLowerCase();
  const items = useMemo(
    () =>
      commits.flatMap((commit, index) => {
        const row = rows[index];
        if (!row) return [];
        if (
          searchText &&
          ![
            commit.subject,
            commit.author,
            commit.sha,
            ...commit.refs.map((ref) => ref.name),
          ].some((value) => value.toLocaleLowerCase().includes(searchText))
        )
          return [];
        return [{ commit, row }];
      }),
    [commits, rows, searchText],
  );

  return (
    <div className="flex h-full min-h-0 min-w-0 flex-col overflow-hidden">
      {onToggleExpanded ? (
        <button
          type="button"
          onClick={onToggleExpanded}
          aria-expanded={expanded}
          aria-label={expanded ? "Collapse graph" : "Expand graph"}
          className={`flex w-full shrink-0 items-center gap-1 px-3 text-left leading-none hover:bg-content/5 ${
            expanded ? "h-7" : "h-full"
          }`}
        >
          <span className="text-[10px] font-semibold tracking-[0.04em] text-content/55 uppercase">
            Graph
          </span>
          {expanded ? (
            <ChevronDown
              className="ml-auto size-3.5 shrink-0 text-content/50"
              strokeWidth={1.75}
            />
          ) : (
            <ChevronRight
              className="ml-auto size-3.5 shrink-0 text-content/50"
              strokeWidth={1.75}
            />
          )}
        </button>
      ) : (
        <HistoryToolbar
          key={cwd}
          cwd={cwd}
          commits={commits}
          loading={loading}
          error={error}
          onRefresh={refresh}
          query={query}
          onQueryChange={(query) => setSearch({ cwd, query })}
        />
      )}
      {expanded ? (
        <div
          ref={lockOverscroll}
          className="min-h-0 min-w-0 flex-1 overflow-x-hidden overflow-y-auto overscroll-none"
        >
          {!cwd || cwd === "~" ? (
            <p className="px-3 py-2 text-[12px] text-content/45">
              No project folder
            </p>
          ) : error && commits.length === 0 ? (
            standalone ? null : (
              <p className="px-3 py-2 text-[12px] text-content/45">{error}</p>
            )
          ) : commits.length === 0 ? (
            <p className="px-3 py-2 text-[12px] text-content/45">
              {loading ? "Loading history…" : "No commits yet"}
            </p>
          ) : items.length === 0 ? (
            <p className="px-3 py-4 text-center text-[12px] text-content/45">
              No matching commits
            </p>
          ) : (
            <ul className="min-w-0 max-w-full">
              {items.map(({ commit, row }) => (
                <HistoryRow
                  key={commit.sha}
                  commit={commit}
                  row={row}
                  active={selectedSha === commit.sha}
                  detailed={standalone}
                  filtered={Boolean(searchText)}
                  onOpen={(pin) => onOpenCommit(commit, pin)}
                />
              ))}
            </ul>
          )}
        </div>
      ) : null}
    </div>
  );
}

function HistoryToolbar({
  cwd,
  commits,
  loading,
  error,
  onRefresh,
  query,
  onQueryChange,
}: {
  cwd: string;
  commits: GitHistoryCommit[];
  loading: boolean;
  error: string | null;
  onRefresh: () => Promise<void>;
  query: string;
  onQueryChange: (query: string) => void;
}) {
  const [searchOpen, setSearchOpen] = useState(false);
  const searchInput = useRef<HTMLInputElement>(null);
  const searchButton = useRef<HTMLButtonElement>(null);
  const wasSearching = useRef(false);
  useEffect(() => {
    if (searchOpen) searchInput.current?.focus();
    else if (wasSearching.current) searchButton.current?.focus();
    wasSearching.current = searchOpen;
  }, [searchOpen]);
  const closeSearch = () => {
    onQueryChange("");
    setSearchOpen(false);
  };
  const head = commits.find((commit) => commit.head);
  const branch = head?.refs.find((ref) => ref.kind === "local")?.name;
  const actionClass =
    "flex size-6 shrink-0 items-center justify-center rounded-md text-content/50 hover:bg-content/5 hover:text-content disabled:pointer-events-none disabled:opacity-40";
  return (
    <div className="shrink-0">
      <PaneHeader compact role="toolbar" aria-label="History">
        {searchOpen ? (
          <>
            <Search aria-hidden className="size-3.5 shrink-0 text-content/45" />
            <input
              ref={searchInput}
              type="text"
              aria-label="Search commits"
              placeholder="Search history…"
              value={query}
              onChange={(event) => onQueryChange(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Escape") {
                  event.preventDefault();
                  event.stopPropagation();
                  closeSearch();
                }
              }}
              className="h-full min-w-0 flex-1 bg-transparent text-[12px] font-normal text-content outline-none placeholder:text-content/35"
            />
            <button
              type="button"
              aria-label="Close history search"
              title="Close search"
              className={actionClass}
              onClick={closeSearch}
            >
              <X className="size-3" />
            </button>
          </>
        ) : (
          <>
            <div className="flex min-w-0 flex-1 items-center gap-1.5">
              <GitBranch className="size-3.5 shrink-0 text-content/45" />
              <span
                className="truncate"
                title={branch ?? (head ? "Detached HEAD" : "History")}
              >
                {branch ?? (head ? head.shortSha : "History")}
              </span>
            </div>
            <div className="flex shrink-0 items-center gap-0.5">
              <button
                ref={searchButton}
                type="button"
                aria-label="Search history"
                title="Search history"
                className={actionClass}
                onClick={() => setSearchOpen(true)}
              >
                <Search className="size-3.5" />
              </button>
              <button
                type="button"
                aria-label="Refresh history"
                title="Refresh history"
                disabled={!cwd || cwd === "~" || loading}
                className={actionClass}
                onClick={() => void onRefresh()}
              >
                <RefreshCw
                  className={`size-3.5 ${loading ? "animate-spin" : ""}`}
                />
              </button>
            </div>
          </>
        )}
      </PaneHeader>
      {error ? (
        <p
          role="alert"
          className="border-b border-stroke px-3 py-2 break-words text-[11px] text-red-400"
        >
          {error}
        </p>
      ) : null}
    </div>
  );
}

function HistoryRow({
  commit,
  row,
  active,
  detailed,
  filtered,
  onOpen,
}: {
  commit: GitHistoryCommit;
  row: HistoryItemViewModel;
  active: boolean;
  detailed: boolean;
  filtered: boolean;
  onOpen: (pin?: boolean) => void;
}) {
  const height = detailed ? HISTORY_ROW_PX : GRAPH_ROW_PX;
  const graph = historyItemGraph(row, height);
  const badge = row.refs.find((ref) => ref.color) ?? row.refs[0];
  const date = new Date(commit.timestamp * 1000);
  const validDate = Number.isFinite(date.getTime());
  return (
    <li className="min-w-0 overflow-visible" style={{ height }}>
      <button
        type="button"
        title={`${commit.shortSha} ${commit.subject}${commit.author ? ` — ${commit.author}` : ""}${validDate ? `\n${date.toLocaleString()}` : ""}${commit.refs.length ? `\n${commit.refs.map((ref) => ref.name).join(", ")}` : ""}`}
        onClick={() => onOpen()}
        onDoubleClick={() => onOpen(true)}
        aria-pressed={active}
        style={{ height }}
        className={`git-history-item flex min-w-0 w-full items-stretch overflow-visible rounded-none text-left ${detailed ? "pr-3 pl-1" : "pr-2"} ${
          row.kind === "HEAD" ? "is-head" : ""
        } ${
          active
            ? "is-selected bg-selection text-content"
            : "text-content hover:bg-content/5"
        }`}
      >
        <svg
          aria-hidden
          className="git-history-graph pointer-events-none block shrink-0 overflow-visible"
          width={filtered ? SWIMLANE_WIDTH * 2 : graph.width}
          height={graph.height}
          overflow="visible"
        >
          {!filtered &&
            graph.paths.map((path, pathIndex) => (
              <path
                key={pathIndex}
                d={path.d}
                fill="none"
                stroke={detailed ? softHistoryColor(path.color) : path.color}
                strokeOpacity={
                  detailed ? "var(--git-history-line-opacity, 0.8)" : undefined
                }
                strokeWidth={
                  detailed
                    ? `var(--git-history-line-width, ${path.strokeWidth}px)`
                    : path.strokeWidth
                }
                strokeLinecap="round"
              />
            ))}
          {graph.circles.map((circle, circleIndex) => (
            <circle
              key={circleIndex}
              cx={filtered ? SWIMLANE_WIDTH : circle.cx}
              cy={circle.cy}
              r={circle.r}
              fill={
                circle.fill
                  ? detailed
                    ? softHistoryColor(circle.fill)
                    : circle.fill
                  : "none"
              }
              strokeWidth={circle.strokeWidth}
            />
          ))}
        </svg>
        {detailed ? (
          <span className="ml-1 flex min-w-0 flex-1 flex-col justify-center gap-0.5 overflow-hidden">
            <span className="flex min-w-0 items-center gap-1">
              <span
                className={`min-w-0 flex-1 truncate text-[12px] leading-4 ${row.kind === "HEAD" ? "font-medium" : ""}`}
              >
                {commit.subject || commit.shortSha}
              </span>
              {badge ? <RefPill refInfo={badge} /> : null}
            </span>
            <span className="flex min-w-0 items-center gap-1.5 text-[10px] leading-3.5 text-content/40">
              <span className="min-w-0 flex-1 truncate">{commit.author}</span>
              {validDate ? (
                <time className="shrink-0" dateTime={date.toISOString()}>
                  {shortDate.format(date)}
                </time>
              ) : null}
              <span className="shrink-0 font-mono text-content/30">
                {commit.shortSha}
              </span>
            </span>
          </span>
        ) : (
          <>
            <span className="ml-1 flex min-w-0 flex-1 items-center overflow-hidden">
              <span
                className={`min-w-0 truncate text-[12px] leading-[22px] ${
                  row.kind === "HEAD" ? "font-semibold" : ""
                }`}
              >
                {commit.subject || commit.shortSha}
              </span>
              {commit.author ? (
                <span className="ml-2 min-w-0 shrink truncate text-[12px] leading-[22px] text-content/45">
                  {commit.author}
                </span>
              ) : null}
            </span>
            {badge ? <RefPill refInfo={badge} /> : null}
          </>
        )}
      </button>
    </li>
  );
}

function RefPill({ refInfo }: { refInfo: GraphRef }) {
  const local = refInfo.kind === "local";
  return (
    <span
      className={`ml-1 flex h-3.5 min-w-0 max-w-[6.5rem] shrink-0 self-center items-center gap-0.5 truncate rounded-full px-1.5 text-[10px] leading-none ${
        refInfo.color ? "" : "bg-content/10 text-content/55"
      }`}
      style={
        refInfo.color
          ? {
              backgroundColor: refInfo.color,
              color: "var(--color-background-base)",
            }
          : undefined
      }
    >
      {local ? (
        <GitBranch className="size-2.5 shrink-0" strokeWidth={2} />
      ) : null}
      <span className="min-w-0 truncate">{refInfo.name}</span>
    </span>
  );
}

function useGitHistory(cwd: string, enabled: boolean) {
  const [commits, setCommits] = useState<GitHistoryCommit[]>(
    () => historyByCwd.get(cwd) ?? [],
  );
  const commitsRef = useRef(commits);
  commitsRef.current = commits;
  const request = useRef(0);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    if (!enabled || !cwd || cwd === "~") return;
    const id = ++request.current;
    setLoading(true);
    setError(null);
    try {
      const next = await gitHistory(cwd);
      if (id !== request.current) return;
      if (!sameHistory(commitsRef.current, next.commits)) {
        historyByCwd.set(cwd, next.commits);
        commitsRef.current = next.commits;
        setCommits(next.commits);
      }
    } catch (reason) {
      if (id === request.current)
        setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      if (id === request.current) setLoading(false);
    }
  }, [cwd, enabled]);

  useEffect(() => {
    setError(null);
    setLoading(false);
    if (!enabled || !cwd || cwd === "~") {
      commitsRef.current = [];
      setCommits([]);
      return;
    }
    const cached = historyByCwd.get(cwd) ?? [];
    commitsRef.current = cached;
    setCommits(cached);
    void load();
    const onResume = () => {
      if (!document.hidden) void load();
    };
    const onGitChanged = () => {
      void load();
    };
    window.addEventListener("focus", onResume);
    document.addEventListener("visibilitychange", onResume);
    const unsub = subscribeGitChanged(onGitChanged);
    return () => {
      request.current++;
      window.removeEventListener("focus", onResume);
      document.removeEventListener("visibilitychange", onResume);
      unsub();
    };
  }, [cwd, enabled, load]);

  return { commits, loading, error, refresh: load };
}

function sameHistory(
  prev: GitHistoryCommit[],
  next: GitHistoryCommit[],
): boolean {
  if (prev.length !== next.length) return false;
  return prev.every((commit, i) => {
    const other = next[i];
    return (
      other &&
      commit.sha === other.sha &&
      commit.subject === other.subject &&
      commit.author === other.author &&
      commit.timestamp === other.timestamp &&
      commit.head === other.head &&
      commit.refs.length === other.refs.length &&
      commit.refs.every(
        (ref, j) =>
          other.refs[j]?.name === ref.name && other.refs[j]?.kind === ref.kind,
      )
    );
  });
}

export const GRAPH_PANEL_MIN = 120;
export const GRAPH_PANEL_DEFAULT = 240;

let graphPanelHeight = GRAPH_PANEL_DEFAULT;

export function loadGraphPanelHeight(): number {
  return graphPanelHeight;
}

export function saveGraphPanelHeight(height: number) {
  graphPanelHeight = height;
}

export function GraphResizeSash({
  height,
  onHeightPaint,
  onHeightCommit,
  maxHeight,
}: {
  height: number;
  onHeightPaint: (height: number) => void;
  onHeightCommit: (height: number) => void;
  maxHeight: () => number;
}) {
  const drag = useRef<{ start: number; size: number } | null>(null);
  const [dragging, setDragging] = useState(false);
  const paintedRef = useRef(height);
  paintedRef.current = height;
  const paintRef = useRef(onHeightPaint);
  paintRef.current = onHeightPaint;
  const commitRef = useRef(onHeightCommit);
  commitRef.current = onHeightCommit;
  const maxRef = useRef(maxHeight);
  maxRef.current = maxHeight;

  const clamp = (value: number) =>
    Math.min(maxRef.current(), Math.max(GRAPH_PANEL_MIN, Math.round(value)));

  const onPointerDown = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) return;
    event.preventDefault();
    event.stopPropagation();
    const handle = event.currentTarget;
    const pointerId = event.pointerId;
    handle.setPointerCapture(pointerId);
    drag.current = { start: event.clientY, size: paintedRef.current };
    setDragging(true);
    const restoreSelection = suppressTextSelection();
    const previousCursor = document.body.style.cursor;
    document.body.style.cursor = "row-resize";
    document.documentElement.classList.add("is-resizing");

    const onMove = (ev: PointerEvent) => {
      if (ev.pointerId !== pointerId || !drag.current) return;
      const next = clamp(drag.current.size - (ev.clientY - drag.current.start));
      paintedRef.current = next;
      paintRef.current(next);
    };

    const stop = () => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      window.removeEventListener("pointercancel", onUp);
      restoreSelection();
      document.body.style.cursor = previousCursor;
      document.documentElement.classList.remove("is-resizing");
      setDragging(false);
      drag.current = null;
      try {
        handle.releasePointerCapture(pointerId);
      } catch {
        /* already released */
      }
      commitRef.current(clamp(paintedRef.current));
    };

    const onUp = (ev: PointerEvent) => {
      if (ev.pointerId !== pointerId) return;
      stop();
    };

    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
    window.addEventListener("pointercancel", onUp);
  };

  return (
    <div
      role="separator"
      aria-orientation="horizontal"
      aria-label="Resize graph"
      aria-valuenow={height}
      className={`z-10 h-1.5 shrink-0 cursor-row-resize touch-none ${
        dragging ? "bg-content/15" : "hover:bg-content/10"
      }`}
      onPointerDown={onPointerDown}
      onDoubleClick={() => commitRef.current(clamp(GRAPH_PANEL_DEFAULT))}
    />
  );
}
