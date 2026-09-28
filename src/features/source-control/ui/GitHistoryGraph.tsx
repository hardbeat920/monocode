import {
  useCallback,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type PointerEvent as ReactPointerEvent,
} from "react";
import { ChevronDown, ChevronRight, GitBranch } from "../../../shared/ui/icons";
import { useLockOverscroll } from "../../../shared/hooks/useLockOverscroll";
import { useHoverCard } from "../../../shared/hooks/useHoverCard";
import { suppressTextSelection } from "../../../shared/lib/drag";
import {
  gitHistory,
  subscribeGitChanged,
  type GitHistoryCommit,
} from "../../../platform/tauri/fs";
import {
  GRAPH_ROW_PX,
  historyItemGraph,
  layoutGitGraph,
  type GraphRef,
  type HistoryItemViewModel,
} from "../model/gitGraph";
import { CommitHoverCard } from "./CommitHoverCard";

type Props = {
  cwd: string;
  enabled: boolean;
  expanded: boolean;
  selectedSha?: string;
  onToggleExpanded: () => void;
  onOpenCommit: (commit: GitHistoryCommit, pin?: boolean) => void;
};

const historyByCwd = new Map<string, GitHistoryCommit[]>();

export function GitHistoryGraph({
  cwd,
  enabled,
  expanded,
  selectedSha,
  onToggleExpanded,
  onOpenCommit,
}: Props) {
  const lockOverscroll = useLockOverscroll<HTMLDivElement>();
  const { commits } = useGitHistory(cwd, enabled && expanded);
  const rows = useMemo(() => layoutGitGraph(commits), [commits]);

  return (
    <div className="flex h-full min-h-0 min-w-0 flex-col overflow-hidden">
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
      {expanded ? (
        <div
          ref={lockOverscroll}
          data-history-scroll
          className="min-h-0 min-w-0 flex-1 overflow-x-hidden overflow-y-auto overscroll-none"
        >
          {!cwd || cwd === "~" ? (
            <p className="px-3 py-2 text-[12px] text-content/45">
              No project folder
            </p>
          ) : commits.length === 0 ? (
            <p className="px-3 py-2 text-[12px] text-content/45">
              No commits yet
            </p>
          ) : (
            <ul className="min-w-0 max-w-full">
              {commits.map((commit, index) => {
                const row = rows[index];
                if (!row) return null;
                return (
                  <HistoryRow
                    key={commit.sha}
                    cwd={cwd}
                    commit={commit}
                    row={row}
                    active={selectedSha === commit.sha}
                    onOpen={(pin) => onOpenCommit(commit, pin)}
                  />
                );
              })}
            </ul>
          )}
        </div>
      ) : null}
    </div>
  );
}

function HistoryRow({
  cwd,
  commit,
  row,
  active,
  onOpen,
}: {
  cwd: string;
  commit: GitHistoryCommit;
  row: HistoryItemViewModel;
  active: boolean;
  onOpen: (pin?: boolean) => void;
}) {
  const graph = historyItemGraph(row);
  const badge = row.refs.find((ref) => ref.color) ?? row.refs[0];
  const anchorRef = useRef<HTMLButtonElement>(null);
  const cardId = useId();
  // `editor.hover.delay`, which defaults to 300. Long enough that sweeping the
  // pointer down the list does not flash a card on every row it passes; VS Code
  // uses a long *hide* delay rather than a long open delay to settle that
  // trade, and the close delay below is the other half of it.
  const hover = useHoverCard({ openDelayMs: 300 });
  // Focus always opens the card, so putting focus back on the row after a
  // dismissal would immediately reopen what the user just closed. This guard
  // makes the dismissal win without depending on the order of the two calls.
  const refocusing = useRef(false);

  const refocusAfterDismiss = useCallback(() => {
    // Not named `row`: that is the history item prop, and an HTMLElement under
    // the same name reads as the wrong thing entirely.
    const element = anchorRef.current;
    if (!element || element === document.activeElement) return;
    refocusing.current = true;
    element.focus();
    // Leave no guard armed if focus did not actually land.
    if (document.activeElement !== element) refocusing.current = false;
  }, []);

  const { openNow, closeNow, open } = hover;
  const openOnFocus = useCallback(() => {
    if (refocusing.current) {
      refocusing.current = false;
      return;
    }
    openNow();
  }, [openNow]);

  // A card belongs to the row it describes. The card is `position: fixed`
  // beside the panel, and `placePopover` keeps it inside the window, so once
  // the row scrolls out of the list the card is left clamped to the viewport,
  // drifting further above the row with every scroll and describing nothing.
  // Close it instead, the way a native title tooltip goes away.
  useEffect(() => {
    if (!open) return;
    const element = anchorRef.current;
    if (!element || typeof IntersectionObserver === "undefined") return;
    const observer = new IntersectionObserver(
      ([entry]) => {
        if (!entry?.isIntersecting) closeNow();
      },
      { root: element.closest("[data-history-scroll]"), threshold: 0 },
    );
    observer.observe(element);
    return () => observer.disconnect();
  }, [open, closeNow]);
  return (
    <li className="min-w-0 overflow-visible" style={{ height: GRAPH_ROW_PX }}>
      <button
        ref={anchorRef}
        type="button"
        data-history-row
        onClick={() => onOpen()}
        onDoubleClick={() => onOpen(true)}
        onMouseEnter={hover.openAfterDelay}
        onMouseLeave={hover.closeAfterDelay}
        onFocus={openOnFocus}
        onKeyDown={(event) => {
          // Focus opens the card immediately, so the card's actions are always
          // mounted and Tab can be handed to them. This costs one extra stop per
          // row: row → card actions → next row. It is the price of reaching the
          // copy actions without a pointer, and the hand-off is unavoidable
          // because the card is portalled out of the list.
          if (event.key !== "Tab" || event.shiftKey) return;
          const firstButton = document
            .getElementById(cardId)
            ?.querySelector("button");
          if (firstButton) {
            event.preventDefault();
            firstButton.focus();
          }
        }}
        onBlur={(event) => {
          const next = event.relatedTarget;
          if (
            next instanceof Node &&
            document.getElementById(cardId)?.contains(next)
          ) {
            return;
          }
          closeNow();
        }}
        // `aria-current` marks which commit is open. It replaced `aria-pressed`,
        // which conflicted with `aria-expanded` on the same button: a row is not
        // a toggle button that happens to also disclose something.
        aria-current={active ? "true" : undefined}
        aria-haspopup="dialog"
        aria-expanded={open}
        aria-controls={open ? cardId : undefined}
        className={`git-history-item flex h-[22px] min-w-0 w-full items-stretch overflow-visible pr-2 text-left ${
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
          width={graph.width}
          height={graph.height}
          overflow="visible"
        >
          {graph.paths.map((path, pathIndex) => (
            <path
              key={pathIndex}
              d={path.d}
              fill="none"
              stroke={path.color}
              strokeWidth={path.strokeWidth}
              strokeLinecap="round"
            />
          ))}
          {graph.circles.map((circle, circleIndex) => (
            <circle
              key={circleIndex}
              cx={circle.cx}
              cy={circle.cy}
              r={circle.r}
              fill={circle.fill ?? "none"}
              strokeWidth={circle.strokeWidth}
            />
          ))}
        </svg>
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
      </button>
      {hover.open ? (
        <CommitHoverCard
          cwd={cwd}
          commit={commit}
          refs={row.refs}
          anchor={anchorRef}
          id={cardId}
          onDismiss={(reason) => {
            hover.closeNow();
            if (reason === "escape") refocusAfterDismiss();
          }}
          onFocusLeave={(next) => {
            if (next !== anchorRef.current) hover.closeNow();
          }}
          onReturnFocus={() => anchorRef.current?.focus()}
          onTabForward={() => focusNextHistoryRow(anchorRef.current)}
          onPointerEnter={hover.cancelClose}
          onPointerLeave={hover.closeAfterDelay}
        />
      ) : null}
    </li>
  );
}

/**
 * Focus the next commit row, so Tab walks row → card → row instead of running
 * off the end of the document. The card is portalled to the body, so DOM order
 * can no longer carry focus from the last action back into the list.
 *
 * Returns false when this is the final row, which lets the caller fall
 * through to the browser's own Tab handling. Rows are found by their shared
 * attribute rather than by `nextElementSibling`, so a non-row element between
 * two rows is skipped instead of swallowing the traversal.
 */
function focusNextHistoryRow(anchor: HTMLElement | null): boolean {
  if (!anchor) return false;
  const rows = anchor
    .closest("ul")
    ?.querySelectorAll<HTMLButtonElement>("[data-history-row]");
  if (!rows) return false;
  const index = Array.prototype.indexOf.call(rows, anchor);
  const next = rows[index + 1];
  if (!next) return false;
  next.focus();
  return true;
}

function RefPill({ refInfo }: { refInfo: GraphRef }) {
  const local = refInfo.kind === "local";
  return (
    <span
      title={refInfo.name}
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

function useGitHistory(
  cwd: string,
  enabled: boolean,
): { commits: GitHistoryCommit[] } {
  const [commits, setCommits] = useState<GitHistoryCommit[]>(
    () => historyByCwd.get(cwd) ?? [],
  );
  const commitsRef = useRef(commits);
  commitsRef.current = commits;

  const load = useCallback(() => {
    if (!enabled || !cwd || cwd === "~") return;
    void gitHistory(cwd)
      .then((next) => {
        const prev = commitsRef.current;
        if (sameHistory(prev, next.commits)) return;
        historyByCwd.set(cwd, next.commits);
        commitsRef.current = next.commits;
        setCommits(next.commits);
      })
      .catch(() => {
        historyByCwd.delete(cwd);
        commitsRef.current = [];
        setCommits([]);
      });
  }, [cwd, enabled]);

  useEffect(() => {
    if (!enabled || !cwd || cwd === "~") {
      commitsRef.current = [];
      setCommits([]);
      return;
    }
    const cached = historyByCwd.get(cwd) ?? [];
    commitsRef.current = cached;
    setCommits(cached);
    load();
    const onResume = () => {
      if (!document.hidden) load();
    };
    window.addEventListener("focus", onResume);
    document.addEventListener("visibilitychange", onResume);
    const unsub = subscribeGitChanged(load);
    return () => {
      window.removeEventListener("focus", onResume);
      document.removeEventListener("visibilitychange", onResume);
      unsub();
    };
  }, [cwd, enabled, load]);

  return { commits };
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
