import {
  revealedFold,
  type FoldReveal,
  type UnifiedBlock,
  type UnifiedLine,
} from "./unifiedDiff";

export const UNIFIED_LINE_PX = 20;
export const UNIFIED_FOLD_PX = 32;
export const UNIFIED_HUNK_PX = 22;
export const UNIFIED_OVERSCAN_PX = 1200;

export type DiffViewRow =
  | { type: "line"; line: UnifiedLine; stage: boolean; height: number }
  | { type: "fold"; id: string; hidden: number; height: number };

export type RowWindow = {
  start: number;
  end: number;
  padTop: number;
  padBottom: number;
};

export type RowLayout = {
  /** Cumulative row positions. `offsets[index]` is the top of that row. */
  offsets: number[];
  totalHeight: number;
};

export function flattenVisibleRows(
  blocks: readonly UnifiedBlock[],
  revealFor: (foldId: string) => FoldReveal | undefined,
  canStageHunk = false,
): DiffViewRow[] {
  const rows: DiffViewRow[] = [];
  for (const block of blocks) {
    if (block.kind === "fold") {
      const split = revealedFold(block.lines.length, revealFor(block.id));
      pushLines(rows, block.lines.slice(0, split.head), false);
      if (split.hidden > 0) {
        rows.push({
          type: "fold",
          id: block.id,
          hidden: split.hidden,
          height: UNIFIED_FOLD_PX,
        });
      }
      if (split.tail > 0) {
        pushLines(
          rows,
          block.lines.slice(block.lines.length - split.tail),
          false,
        );
      }
      continue;
    }
    for (const line of block.lines) {
      const pos = line.pos ?? block.pos;
      const stage =
        canStageHunk &&
        (line.kind === "add" || line.kind === "del") &&
        pos != null;
      rows.push({
        type: "line",
        line: stage && line.pos == null && pos != null ? { ...line, pos } : line,
        stage,
        height: line.kind === "hunk" ? UNIFIED_HUNK_PX : UNIFIED_LINE_PX,
      });
    }
  }
  return rows;
}

export function rowsHeight(rows: readonly DiffViewRow[]): number {
  let height = 0;
  for (const row of rows) height += row.height;
  return height;
}

export function layoutRows(rows: readonly DiffViewRow[]): RowLayout {
  const offsets = new Array<number>(rows.length + 1);
  offsets[0] = 0;
  for (let index = 0; index < rows.length; index += 1) {
    offsets[index + 1] = offsets[index] + rows[index].height;
  }
  return { offsets, totalHeight: offsets[rows.length] ?? 0 };
}

export function windowRows(
  rows: readonly DiffViewRow[],
  viewTop: number,
  viewBottom: number,
  overscan = UNIFIED_OVERSCAN_PX,
  layout = layoutRows(rows),
): RowWindow {
  const total = layout.totalHeight;
  if (rows.length === 0) {
    return { start: 0, end: 0, padTop: 0, padBottom: 0 };
  }
  const from = viewTop - overscan;
  const to = viewBottom + overscan;
  if (to <= 0) {
    return { start: 0, end: 0, padTop: 0, padBottom: total };
  }
  if (from >= total) {
    return {
      start: rows.length,
      end: rows.length,
      padTop: total,
      padBottom: 0,
    };
  }

  const start = Math.max(
    0,
    Math.min(rows.length, upperBound(layout.offsets, from) - 1),
  );
  const end = Math.max(
    start,
    Math.min(rows.length, lowerBound(layout.offsets, to)),
  );
  return {
    start,
    end,
    padTop: layout.offsets[start] ?? total,
    padBottom: total - (layout.offsets[end] ?? total),
  };
}

function lowerBound(values: readonly number[], target: number): number {
  let low = 0;
  let high = values.length;
  while (low < high) {
    const middle = (low + high) >>> 1;
    if (values[middle] < target) low = middle + 1;
    else high = middle;
  }
  return low;
}

function upperBound(values: readonly number[], target: number): number {
  let low = 0;
  let high = values.length;
  while (low < high) {
    const middle = (low + high) >>> 1;
    if (values[middle] <= target) low = middle + 1;
    else high = middle;
  }
  return low;
}

function pushLines(
  rows: DiffViewRow[],
  lines: readonly UnifiedLine[],
  canStage: boolean,
) {
  for (const line of lines) {
    const stage =
      canStage && (line.kind === "add" || line.kind === "del") && line.pos != null;
    rows.push({
      type: "line",
      line,
      stage,
      height: line.kind === "hunk" ? UNIFIED_HUNK_PX : UNIFIED_LINE_PX,
    });
  }
}
