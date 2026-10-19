import {
  Check,
  ChevronDown,
  ChevronRight,
  ChevronUp,
  FoldVertical,
  MessageSquarePlus,
  Undo2,
  UnfoldVertical,
} from "../../../shared/ui/icons";
import {
  memo,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
} from "react";
import { FileTypeIcon } from "../../files/ui/FileTypeIcon";
import {
  lockOverscroll,
  useLockOverscroll,
} from "../../../shared/hooks/useLockOverscroll";
import { formatInteger } from "../../../shared/lib/numbers";
import { basename } from "../../../platform/tauri/fs";
import type { SyntaxToken } from "../../files/editor/syntaxTokens";
import {
  DiffCommentComposer,
  type DiffCommentComposerTarget,
} from "./DiffCommentComposer";
import { PaneHeader } from "../../../shared/ui/PaneHeader";
import type { UnifiedLine } from "../model/unifiedDiff";
import { windowRows } from "../model/unifiedDiffWindow";
import {
  DIFF_FILE_HEADER_PX,
  ReviewDocumentBuilder,
  type ReviewDocument,
  type ReviewRow,
  type UnifiedDiffFileModel,
} from "../model/unifiedDiffDocument";
import {
  cachedDiffHighlight,
  highlightDiffInBackground,
} from "../model/diffHighlight";
import { reuseUnchangedById } from "../model/stableDiff";
import { useDiffViewport } from "../hooks/useDiffViewport";
import "./unifiedDiff.css";

export type { UnifiedDiffFileModel } from "../model/unifiedDiffDocument";
type InitialExpansion = "all" | "first" | "none";
type Props = {
  files: UnifiedDiffFileModel[];
  truncated?: boolean;
  fileCount?: number;
  focusPath?: string;
  focusId?: string;
  busyId?: string | null;
  totals?: { additions: number; deletions: number };
  visible?: boolean;
  fill?: boolean;
  fileLayout?: "stacked" | "cards";
  initialExpansion?: InitialExpansion;
  onStageFile?: (id: string) => void;
  onDiscardFile?: (id: string) => void;
  onStageHunk?: (id: string, pos: number) => void;
};
type PendingJump = { kind: "file" | "hunk"; key: string };
type Actions = {
  toggle: (id: string) => void;
  stage: (id: string) => void;
  discard: (id: string) => void;
  stageHunk: (id: string, pos: number) => void;
  comment: (path: string, line: UnifiedLine, anchor: DOMRect) => void;
};
const EMPTY_OPEN = new Set<string>();

export function UnifiedDiffView({
  files,
  truncated,
  fileCount,
  focusPath,
  focusId,
  busyId,
  totals,
  visible: isVisible = true,
  fill = true,
  fileLayout = "stacked",
  initialExpansion = "all",
  onStageFile,
  onDiscardFile,
  onStageHunk,
}: Props) {
  const previousFiles = useRef<UnifiedDiffFileModel[]>([]);
  const stableFiles = useMemo(() => {
    const stable = reuseUnchangedById(previousFiles.current, files);
    previousFiles.current = stable;
    return stable;
  }, [files]);
  const byId = useMemo(
    () => new Map(stableFiles.map((file) => [file.id, file])),
    [stableFiles],
  );
  const fileKey = useMemo(
    () => stableFiles.map((file) => file.id).join("\n"),
    [stableFiles],
  );
  const [open, setOpen] = useState(() =>
    initialOpen(stableFiles, initialExpansion),
  );
  const known = useRef({
    ids: new Set(stableFiles.map((file) => file.id)),
    mode: initialExpansion,
  });
  const builder = useRef<ReviewDocumentBuilder | null>(null);
  if (!builder.current) builder.current = new ReviewDocumentBuilder();
  const document = useMemo(
    () =>
      builder.current!.build(
        stableFiles,
        open,
        Boolean(onStageHunk),
        fileLayout === "cards",
      ),
    [stableFiles, open, onStageHunk, fileLayout],
  );
  const indices = useMemo(
    () => new Map(document.rows.map((row, index) => [row.key, index])),
    [document],
  );
  const fileSections = useMemo(() => {
    const starts = [...document.fileRows.entries()];
    return starts.map(([fileId, start], index) => ({
      fileId,
      start,
      end: starts[index + 1]?.[1] ?? document.rows.length,
    }));
  }, [document]);
  const scroller = useRef<HTMLDivElement | null>(null);
  const content = useRef<HTMLDivElement | null>(null);
  const horizontalBar = useRef<HTMLDivElement | null>(null);
  const horizontalPosition = useRef(0);
  const bindCodeScroller = useCallback((node: HTMLDivElement | null) => {
    if (!node) return;
    node.scrollLeft = horizontalPosition.current;
    return lockOverscroll(node, "horizontal");
  }, []);
  const bindHorizontalBar = useCallback((node: HTMLDivElement | null) => {
    horizontalBar.current = node;
    if (!node) return;
    const detach = lockOverscroll(node, "horizontal");
    return () => {
      detach();
      horizontalBar.current = null;
    };
  }, []);
  const syncHorizontal = useCallback((source: HTMLDivElement) => {
    const left = source.scrollLeft;
    if (Math.abs(left - horizontalPosition.current) < 0.5) return;
    horizontalPosition.current = left;
    for (const pane of content.current?.querySelectorAll<HTMLDivElement>(
      ".diff-review-code-scroll",
    ) ?? [])
      if (pane !== source) pane.scrollLeft = left;
    if (horizontalBar.current && horizontalBar.current !== source)
      horizontalBar.current.scrollLeft = left;
  }, []);
  const hoveredGutter = useRef<HTMLElement | null>(null);
  const clearHover = () => {
    hoveredGutter.current?.classList.remove("is-hovered");
    hoveredGutter.current = null;
  };
  const lockReviewOverscroll = useLockOverscroll<HTMLDivElement>();
  const bindScroller = useCallback(
    (node: HTMLDivElement | null) => {
      scroller.current = node;
      lockReviewOverscroll(fill ? node : null);
    },
    [fill, lockReviewOverscroll],
  );
  const {
    viewport,
    current: currentViewport,
    scrollTo,
    measure,
  } = useDiffViewport(
    content,
    scroller,
    fill,
    isVisible && stableFiles.length > 0,
  );
  const range = isVisible
    ? windowRows(
        document.rows,
        viewport.top,
        viewport.bottom,
        undefined,
        document,
      )
    : { start: 0, end: 0, padTop: 0, padBottom: document.totalHeight };
  const visible = document.rows.slice(range.start, range.end);
  const sections = fileSections.filter(
    (section) => section.end > range.start && section.start < range.end,
  );
  const visibleFiles = [
    ...new Set(
      visible.filter((row) => row.type === "line").map((row) => row.fileId),
    ),
  ].join("\n");
  useVisibleHighlights(stableFiles, visibleFiles);
  const [pending, setPending] = useState<PendingJump | null>(null);
  const pendingRef = useRef(pending);
  pendingRef.current = pending;
  const [comment, setComment] = useState<{
    path: string;
    target: DiffCommentComposerTarget;
  } | null>(null);
  const latest = useRef({ onStageFile, onDiscardFile, onStageHunk });
  latest.current = { onStageFile, onDiscardFile, onStageHunk };
  const actions = useMemo<Actions>(
    () => ({
      toggle: (id) =>
        setOpen((current) => {
          const next = new Set(current);
          if (next.has(id)) next.delete(id);
          else next.add(id);
          return next;
        }),
      stage: (id) => latest.current.onStageFile?.(id),
      discard: (id) => latest.current.onDiscardFile?.(id),
      stageHunk: (id, pos) => latest.current.onStageHunk?.(id, pos),
      comment: (path, line, anchor) =>
        setComment({ path, target: { line, anchor } }),
    }),
    [],
  );

  // Preserve deliberate folds when a file is staged, removed, or added.
  useLayoutEffect(() => {
    const previous = known.current;
    const ids = new Set(stableFiles.map((file) => file.id));
    setOpen((current) => {
      if (previous.mode !== initialExpansion)
        return initialOpen(stableFiles, initialExpansion);
      const next = new Set([...current].filter((id) => ids.has(id)));
      if (initialExpansion === "all")
        for (const id of ids) if (!previous.ids.has(id)) next.add(id);
      if (
        initialExpansion === "first" &&
        previous.ids.size === 0 &&
        stableFiles[0]
      )
        next.add(stableFiles[0].id);
      return sameSet(current, next) ? current : next;
    });
    known.current = { ids, mode: initialExpansion };
  }, [fileKey, initialExpansion]);

  const resolvedFocus =
    focusId ??
    stableFiles.find((file) => file.path === focusPath || file.id === focusPath)
      ?.id;
  useLayoutEffect(() => {
    if (!resolvedFocus) return;
    setOpen((current) =>
      current.has(resolvedFocus)
        ? current
        : new Set(current).add(resolvedFocus),
    );
    setPending({ kind: "file", key: resolvedFocus });
  }, [resolvedFocus]);

  const previousDocument = useRef<ReviewDocument | null>(null);
  useLayoutEffect(() => {
    if (!isVisible) return;
    const previous = previousDocument.current;
    previousDocument.current = document;
    if (pending) {
      const target =
        pending.kind === "file"
          ? document.fileRows.get(pending.key)
          : indices.get(pending.key);
      if (target != null) {
        scrollTo(
          Math.max(
            0,
            document.offsets[target] -
              (pending.kind === "hunk" ? DIFF_FILE_HEADER_PX + 8 : 0),
          ),
        );
        const fileId = document.rows[target]?.fileId;
        if (fileId && byId.get(fileId)?.emptyMessage !== "Loading…")
          setPending(null);
      }
      return;
    }
    // Padding changes from virtualization never become browser scroll anchors.
    // Data changes above the reader are anchored to the actual source row.
    if (previous && previous !== document && previous.rows.length) {
      const top = currentViewport.current.top;
      const anchor = windowRows(previous.rows, top, top + 1, 0, previous).start;
      const row = previous.rows[anchor];
      if (row) {
        const next = indices.get(row.key) ?? document.fileRows.get(row.fileId);
        if (next != null) {
          const delta = indices.has(row.key)
            ? top - previous.offsets[anchor]
            : 0;
          const nextTop = document.offsets[next] + delta;
          if (Math.abs(nextTop - top) > 0.5) scrollTo(nextTop);
        }
      }
    }
    measure();
  }, [
    document,
    pending,
    byId,
    indices,
    scrollTo,
    measure,
    currentViewport,
    isVisible,
  ]);

  const currentHunk = currentHunkIndex(
    document,
    viewport.top + DIFF_FILE_HEADER_PX + 8,
  );
  const navigate = useCallback(
    (direction: number) => {
      const current = currentHunkIndex(
        document,
        currentViewport.current.top + DIFF_FILE_HEADER_PX + 8,
      );
      const index = Math.max(
        0,
        Math.min(document.hunks.length - 1, current + direction),
      );
      const target = document.hunks[index];
      if (!target) return;
      setOpen((current) =>
        current.has(target.fileId)
          ? current
          : new Set(current).add(target.fileId),
      );
      setPending({ kind: "hunk", key: target.key });
    },
    [document, currentViewport],
  );
  const stopJump = () => {
    if (pendingRef.current) setPending(null);
  };
  const count = fileCount ?? stableFiles.length;
  const additions =
    totals?.additions ??
    stableFiles.reduce((sum, file) => sum + file.additions, 0);
  const deletions =
    totals?.deletions ??
    stableFiles.reduce((sum, file) => sum + file.deletions, 0);
  const gutterWidth = document.numberWidth * 2 + 24 + 3;
  const surfaceStyle = {
    "--diff-number-width": `${document.numberWidth}px`,
    "--diff-gutter-width": `${gutterWidth}px`,
    "--diff-code-min-width": `calc(${document.maxColumns + 2}ch + 24px)`,
  } as CSSProperties;
  const renderRow = (row: ReviewRow, lane: "gutter" | "code" = "code") => {
    const file = byId.get(row.fileId)!;
    return (
      <ReviewRowView
        key={row.key}
        row={row}
        lane={lane}
        file={file}
        expanded={open.has(file.id)}
        focused={resolvedFocus === file.id}
        busy={busyId === file.id}
        cards={fileLayout === "cards"}
        actions={actions}
        stageable={Boolean(onStageFile)}
        discardable={Boolean(onDiscardFile)}
        stickyHeader={fill && fileLayout === "stacked"}
        tokens={
          row.type === "line"
            ? cachedDiffHighlight(file)?.get(row.line)
            : undefined
        }
      />
    );
  };
  if (stableFiles.length === 0)
    return (
      <p className="px-4 py-6 text-[13px] text-content/45">No file changes</p>
    );

  return (
    <div
      tabIndex={-1}
      className={
        fill
          ? "diff-review flex h-full min-h-0 min-w-0 flex-1 flex-col overflow-hidden"
          : "diff-review min-w-0"
      }
      style={surfaceStyle}
      onWheelCapture={stopJump}
      onTouchStartCapture={stopJump}
      onPointerDownCapture={(event) => {
        stopJump();
        if (
          !(event.target as HTMLElement).closest(
            "input, textarea, [contenteditable=true]",
          )
        )
          event.currentTarget.focus({ preventScroll: true });
      }}
      onKeyDown={(event) => {
        if (
          event.altKey &&
          !event.metaKey &&
          !event.ctrlKey &&
          !event.shiftKey &&
          (event.key === "ArrowDown" || event.key === "ArrowUp") &&
          !(event.target as HTMLElement).closest(
            "input, textarea, [contenteditable=true]",
          )
        ) {
          event.preventDefault();
          event.stopPropagation();
          navigate(event.key === "ArrowDown" ? 1 : -1);
        }
      }}
    >
      <PaneHeader compact role="toolbar" aria-label="Diff tools">
        <span className="shrink-0 text-content/65">
          {count === 1 ? "1 file" : `${count} files`}
        </span>
        <DiffCounts additions={additions} deletions={deletions} />
        <div className="ml-auto flex shrink-0 items-center gap-0.5">
          <IconButton
            title="Previous hunk"
            hint="Previous hunk (Alt+↑)"
            disabled={document.hunks.length === 0 || currentHunk <= 0}
            onClick={() => navigate(-1)}
          >
            <ChevronUp className="size-3.5" />
          </IconButton>
          <span
            className="min-w-9 select-none text-center text-[10px] font-normal tabular-nums text-content/40"
            aria-label="Hunk position"
          >
            {document.hunks.length
              ? `${currentHunk + 1}/${document.hunks.length}`
              : "0/0"}
          </span>
          <IconButton
            title="Next hunk"
            hint="Next hunk (Alt+↓)"
            disabled={
              document.hunks.length === 0 ||
              currentHunk >= document.hunks.length - 1
            }
            onClick={() => navigate(1)}
          >
            <ChevronDown className="size-3.5" />
          </IconButton>
          <span className="mx-1 h-3 w-px bg-stroke" />
          <IconButton
            title="Expand all files"
            onClick={() => setOpen(new Set(stableFiles.map((file) => file.id)))}
          >
            <UnfoldVertical className="size-3.5" />
          </IconButton>
          <IconButton
            title="Collapse all files"
            disabled={open.size === 0}
            onClick={() => setOpen(EMPTY_OPEN)}
          >
            <FoldVertical className="size-3.5" />
          </IconButton>
        </div>
      </PaneHeader>
      {truncated ? (
        <p className="border-b border-stroke px-3 py-2 text-[12px] text-content/45">
          Diff is too large to display in full. Some patches are unavailable.
        </p>
      ) : null}
      <div className={fill ? "relative min-h-0 flex-1" : "relative"}>
        <div
          ref={bindScroller}
          role="region"
          aria-label="Unified diff"
          tabIndex={0}
          className={`unified-diff diff-review-scroll outline-none ${fill ? "h-full min-h-0 overflow-y-auto overflow-x-hidden overscroll-none" : "overflow-x-clip"}`}
        >
          <div
            ref={content}
            className="diff-review-content font-mono"
            style={{
              height: document.totalHeight,
            }}
          >
            <div
              style={{
                paddingTop: sections.length
                  ? document.offsets[sections[0].start]
                  : range.padTop,
                paddingBottom: sections.length
                  ? document.totalHeight -
                    document.offsets[sections[sections.length - 1].end]
                  : range.padBottom,
              }}
            >
              {sections.map((section) => {
                // Preserve the file's full height for native sticky constraints;
                // only its source rows inside the shared window are mounted.
                const start = Math.max(section.start + 1, range.start);
                const end = Math.max(start, Math.min(section.end, range.end));
                return (
                  <section key={section.fileId}>
                    {renderRow(document.rows[section.start])}
                    <div
                      className="diff-review-file-body"
                      onPointerOver={(event) => {
                        const row = (
                          event.target as Element
                        ).closest<HTMLElement>("[data-diff-row]");
                        const key = row?.dataset.diffRow;
                        const gutter = key
                          ? event.currentTarget.querySelector<HTMLElement>(
                              `[data-diff-gutter-row="${CSS.escape(key)}"]`,
                            )
                          : null;
                        if (gutter === hoveredGutter.current) return;
                        clearHover();
                        gutter?.classList.add("is-hovered");
                        hoveredGutter.current = gutter;
                      }}
                      onPointerLeave={clearHover}
                    >
                      <div
                        className="diff-review-gutter-lane select-none"
                        style={{
                          paddingTop:
                            document.offsets[start] -
                            document.offsets[section.start + 1],
                          paddingBottom:
                            document.offsets[section.end] -
                            document.offsets[end],
                        }}
                      >
                        {document.rows
                          .slice(start, end)
                          .map((row) => renderRow(row, "gutter"))}
                      </div>
                      <div
                        ref={bindCodeScroller}
                        className="diff-review-code-scroll"
                        onScroll={(event) =>
                          syncHorizontal(event.currentTarget)
                        }
                      >
                        <div
                          className="diff-review-code-lane"
                          style={{
                            paddingTop:
                              document.offsets[start] -
                              document.offsets[section.start + 1],
                            paddingBottom:
                              document.offsets[section.end] -
                              document.offsets[end],
                          }}
                        >
                          {document.rows
                            .slice(start, end)
                            .map((row) => renderRow(row, "code"))}
                        </div>
                      </div>
                    </div>
                  </section>
                );
              })}
            </div>
          </div>
        </div>
      </div>
      <div
        ref={bindHorizontalBar}
        className="diff-review-horizontal-scroll font-mono"
        role="region"
        aria-label="Scroll diff horizontally"
        tabIndex={0}
        onScroll={(event) => syncHorizontal(event.currentTarget)}
      >
        <div
          style={{
            minWidth:
              "calc(var(--diff-code-min-width) + var(--diff-gutter-width))",
            height: 1,
          }}
        />
      </div>
      {comment ? (
        <DiffCommentComposer
          path={comment.path}
          target={comment.target}
          onDismiss={() => setComment(null)}
        />
      ) : null}
    </div>
  );
}

const ReviewRowView = memo(function ReviewRowView({
  row,
  lane,
  file,
  expanded,
  focused,
  busy,
  cards,
  actions,
  stageable,
  discardable,
  stickyHeader,
  tokens,
}: {
  row: ReviewRow;
  lane: "gutter" | "code";
  file: UnifiedDiffFileModel;
  expanded: boolean;
  focused: boolean;
  busy: boolean;
  cards: boolean;
  actions: Actions;
  stageable: boolean;
  discardable: boolean;
  stickyHeader: boolean;
  tokens?: SyntaxToken[];
}) {
  const style = { height: row.height };
  if (row.type === "gap")
    return <div className="diff-review-card-gap" style={style} />;
  if (row.type === "file")
    return (
      <div
        data-diff-file={file.path}
        data-diff-file-id={file.id}
        className={`diff-review-file-slot ${stickyHeader ? "is-sticky" : ""} ${expanded ? "is-expanded" : ""} ${cards ? "diff-review-card-header" : ""} ${focused ? "is-focused" : ""}`}
        style={style}
      >
        <FileHeader
          file={file}
          expanded={expanded}
          busy={busy}
          actions={actions}
          stageable={stageable}
          discardable={discardable}
          sticky={stickyHeader}
        />
      </div>
    );
  if (row.type === "line")
    return (
      <CodeLine
        row={row}
        lane={lane}
        file={file}
        actions={actions}
        tokens={tokens}
        busy={busy}
        cards={cards}
      />
    );
  const className = `diff-review-fixed-row ${cards ? "diff-review-card-row" : ""}`;
  if (row.type === "notice")
    return (
      <div className={className} style={style}>
        {lane === "code" ? (
          <span className="px-3 text-[12px] text-content/45">{row.text}</span>
        ) : null}
      </div>
    );
  if (row.type === "hunk")
    return (
      <div
        data-diff-hunk={lane === "code" ? row.key : undefined}
        className={`${className} diff-review-hunk`}
        style={style}
      >
        {lane === "gutter" ? (
          <span className="diff-review-hunk-columns select-none" aria-hidden>
            <span>Old</span>
            <span>New</span>
          </span>
        ) : (
          <span className="min-w-0 truncate px-3 font-mono text-[11px] text-content/50">
            {row.text}
          </span>
        )}
      </div>
    );
  return null;
});

const FileHeader = memo(function FileHeader({
  file,
  expanded,
  busy,
  actions,
  stageable,
  discardable,
  sticky = false,
}: {
  file: UnifiedDiffFileModel;
  expanded: boolean;
  busy: boolean;
  actions: Actions;
  stageable: boolean;
  discardable: boolean;
  sticky?: boolean;
}) {
  const name = basename(file.path);
  const directory = file.label.endsWith(name)
    ? file.label.slice(0, -name.length).replace(/\/$/, "")
    : "";
  const Chevron = expanded ? ChevronDown : ChevronRight;
  return (
    <div
      data-sticky-file={sticky ? file.id : undefined}
      className="diff-review-file-header"
      style={{ height: DIFF_FILE_HEADER_PX }}
    >
      <button
        type="button"
        aria-label={`${expanded ? "Collapse" : "Expand"} ${file.label}`}
        aria-expanded={expanded}
        onClick={() => actions.toggle(file.id)}
        className="flex h-full min-w-0 flex-1 cursor-pointer items-center gap-2 text-left"
        title={file.label}
      >
        <Chevron className="size-3.5 shrink-0 text-content/40" />
        <FileTypeIcon name={name} isDir={false} size={15} />
        <span className="min-w-0 truncate text-[12px] font-medium text-content/90">
          {directory ? name : file.label}
        </span>
        {directory ? (
          <span className="min-w-0 truncate text-[11px] font-normal text-content/40">
            {directory}
          </span>
        ) : null}
      </button>
      <DiffCounts additions={file.additions} deletions={file.deletions} />
      {file.canDiscard && discardable ? (
        <IconButton
          title="Discard file"
          disabled={busy}
          onClick={() => actions.discard(file.id)}
        >
          <Undo2 className="size-3.5" />
        </IconButton>
      ) : null}
      {file.canStage && stageable ? (
        <IconButton
          title="Stage file"
          disabled={busy}
          onClick={() => actions.stage(file.id)}
        >
          <Check className="size-3.5" />
        </IconButton>
      ) : null}
    </div>
  );
});

const CodeLine = memo(function CodeLine({
  row,
  lane,
  file,
  actions,
  tokens,
  busy,
  cards,
}: {
  row: Extract<ReviewRow, { type: "line" }>;
  lane: "gutter" | "code";
  file: UnifiedDiffFileModel;
  actions: Actions;
  tokens?: SyntaxToken[];
  busy: boolean;
  cards: boolean;
}) {
  const { line } = row;
  const added = line.kind === "add";
  const removed = line.kind === "del";
  const number = removed ? line.oldNumber : line.newNumber;
  return (
    <div
      data-diff-line={lane === "code" ? "" : undefined}
      data-diff-gutter-row={lane === "gutter" ? row.key : undefined}
      data-diff-row={row.key}
      className={`diff-review-line ${added ? "is-add" : removed ? "is-del" : "is-context"} ${row.stagePos != null ? "is-stageable" : ""} ${cards ? "diff-review-card-row" : ""}`}
      style={{ height: row.height }}
    >
      {lane === "gutter" ? (
        <div className="diff-review-gutter select-none">
          <span className="diff-review-accent" aria-hidden="true" />
          <span
            className="diff-review-number diff-review-number-old"
            aria-hidden="true"
          >
            {line.oldNumber ?? ""}
          </span>
          <span className="diff-review-number" aria-hidden="true">
            {line.newNumber ?? ""}
          </span>
          <span
            data-diff-marker
            className="diff-review-marker"
            aria-hidden="true"
          >
            {added ? "+" : removed ? "−" : ""}
          </span>
          <div className="diff-review-line-actions">
            <button
              type="button"
              title={`Comment on line ${number ?? ""}`}
              aria-label={`Comment on line ${number ?? ""}`}
              onClick={(event) =>
                actions.comment(
                  file.path,
                  line,
                  event.currentTarget.getBoundingClientRect(),
                )
              }
              className="diff-review-line-action"
            >
              <MessageSquarePlus className="size-3" />
            </button>
            {row.stagePos != null ? (
              <button
                type="button"
                aria-label="Stage hunk"
                title="Stage hunk"
                disabled={busy}
                onClick={() => actions.stageHunk(file.id, row.stagePos!)}
                className="diff-review-line-action"
              >
                <Check className="size-3" />
              </button>
            ) : null}
          </div>
        </div>
      ) : (
        <>
          {added || removed ? (
            <span className="sr-only select-none">
              {added ? "Added: " : "Removed: "}
            </span>
          ) : null}
          <span data-diff-code className="diff-review-code">
            {renderCode(line.text, tokens)}
          </span>
        </>
      )}
    </div>
  );
});

function renderCode(text: string, tokens?: SyntaxToken[]) {
  if (!tokens?.length || (tokens.length === 1 && !tokens[0].color)) return text;
  return tokens.map((piece, index) => (
    <span key={index} style={piece.color ? { color: piece.color } : undefined}>
      {piece.text}
    </span>
  ));
}
function DiffCounts({
  additions,
  deletions,
}: {
  additions: number;
  deletions: number;
}) {
  if (!additions && !deletions) return null;
  return (
    <span className="flex shrink-0 items-center gap-1.5 font-sans text-[11px] font-medium tabular-nums">
      {additions > 0 ? (
        <span className="text-diff-add-fg">+{formatInteger(additions)}</span>
      ) : null}
      {deletions > 0 ? (
        <span className="text-diff-del-fg">−{formatInteger(deletions)}</span>
      ) : null}
    </span>
  );
}
function IconButton({
  title,
  hint,
  disabled,
  onClick,
  children,
}: {
  title: string;
  hint?: string;
  disabled?: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      title={hint ?? title}
      aria-label={title}
      disabled={disabled}
      onClick={onClick}
      className="grid size-6 shrink-0 cursor-pointer place-items-center rounded-md text-content/45 hover:bg-content/5 hover:text-content disabled:cursor-default disabled:opacity-30"
    >
      {children}
    </button>
  );
}
function initialOpen(
  files: readonly UnifiedDiffFileModel[],
  mode: InitialExpansion,
) {
  return new Set(
    mode === "all"
      ? files.map((file) => file.id)
      : mode === "first" && files[0]
        ? [files[0].id]
        : [],
  );
}
function sameSet(a: ReadonlySet<string>, b: ReadonlySet<string>) {
  return a.size === b.size && [...a].every((key) => b.has(key));
}
function currentHunkIndex(document: ReviewDocument, top: number) {
  let found = -1;
  let first = -1;
  for (let index = 0; index < document.hunks.length; index++) {
    const hunk = document.hunks[index];
    if (hunk.rowIndex == null) continue;
    if (first === -1) first = index;
    if (document.offsets[hunk.rowIndex] > top + 0.5) break;
    found = index;
  }
  return found === -1 ? first : found;
}
function useVisibleHighlights(
  files: UnifiedDiffFileModel[],
  visibleKey: string,
) {
  const requests = useRef(
    new Map<
      string,
      {
        blocks: UnifiedDiffFileModel["blocks"];
        controller: AbortController;
      }
    >(),
  );
  const [, repaint] = useState(0);
  useEffect(() => {
    const visible = new Set(visibleKey.split("\n"));
    const currentFiles = new Map(files.map((file) => [file.id, file]));
    for (const [id, job] of requests.current) {
      if (!visible.has(id) || currentFiles.get(id)?.blocks !== job.blocks) {
        job.controller.abort();
        requests.current.delete(id);
      }
    }
    for (const file of files) {
      if (
        !visible.has(file.id) ||
        requests.current.has(file.id) ||
        cachedDiffHighlight(file)
      )
        continue;
      const controller = new AbortController();
      requests.current.set(file.id, {
        blocks: file.blocks,
        controller,
      });
      void highlightDiffInBackground(file, controller.signal)
        .then(() => {
          if (!controller.signal.aborted) repaint((current) => current + 1);
        })
        .catch(() => {});
    }
  }, [files, visibleKey]);
  useEffect(
    () => () => {
      for (const request of requests.current.values())
        request.controller.abort();
      requests.current.clear();
    },
    [],
  );
}
