import {
  layoutRows,
  UNIFIED_HUNK_PX,
  UNIFIED_LINE_PX,
  type RowLayout,
} from "./unifiedDiffWindow";
import type { UnifiedBlock, UnifiedLine } from "./unifiedDiff";

export type UnifiedDiffFileModel = {
  id: string;
  path: string;
  label: string;
  binary?: boolean;
  tooLarge?: boolean;
  emptyMessage?: string;
  additions: number;
  deletions: number;
  blocks: UnifiedBlock[];
  canStage?: boolean;
  canDiscard?: boolean;
  canStageHunk?: boolean;
};

export const DIFF_FILE_HEADER_PX = 40;
export const DIFF_FILE_GAP_PX = 8;

type RowBase = { key: string; fileId: string; height: number };
export type ReviewRow = RowBase &
  (
    | { type: "file" }
    | { type: "line"; line: UnifiedLine; stagePos?: number }
    | { type: "hunk"; text: string }
    | { type: "notice"; text: string }
    | { type: "gap" }
  );
export type ReviewHunk = {
  key: string;
  fileId: string;
  rowIndex: number | null;
};
export type ReviewDocument = RowLayout & {
  rows: ReviewRow[];
  hunks: ReviewHunk[];
  fileRows: Map<string, number>;
  maxColumns: number;
  numberWidth: number;
};

type CachedBody = {
  blocks: UnifiedBlock[];
  message: string | undefined;
  stageable: boolean;
  rows: ReviewRow[];
  columns: number;
  digits: number;
};

/** File bodies survive unrelated file loads and scrolls. Only changed blocks,
 * notices, or staging availability need to be flattened again. */
export class ReviewDocumentBuilder {
  private bodies = new Map<string, CachedBody>();

  build(
    files: readonly UnifiedDiffFileModel[],
    open: ReadonlySet<string>,
    canStage: boolean,
    cards = false,
  ): ReviewDocument {
    const rows: ReviewRow[] = [];
    const hunks: ReviewHunk[] = [];
    const fileRows = new Map<string, number>();
    const keep = new Set<string>();
    let maxColumns = 40;
    let maxDigits = 3;
    for (const file of files) {
      keep.add(file.id);
      const message = file.binary
        ? "Binary file changed"
        : file.tooLarge
          ? "Diff is too large to display"
          : (file.emptyMessage ??
            (file.blocks.length ? undefined : "No textual diff"));
      const expanded = open.has(file.id);
      const stageable = Boolean(canStage && file.canStageHunk);
      let body = this.bodies.get(file.id);
      if (
        !body ||
        body.blocks !== file.blocks ||
        body.message !== message ||
        body.stageable !== stageable
      ) {
        body = buildBody(file, message, stageable);
        this.bodies.set(file.id, body);
      }
      fileRows.set(file.id, rows.length);
      rows.push({
        type: "file",
        key: `file:${file.id}`,
        fileId: file.id,
        height: DIFF_FILE_HEADER_PX,
      });
      const start = rows.length;
      for (let index = 0; index < body.rows.length; index++) {
        const row = body.rows[index];
        if (row.type === "hunk")
          hunks.push({
            key: row.key,
            fileId: file.id,
            rowIndex: expanded ? start + index : null,
          });
      }
      if (expanded) {
        for (const row of body.rows) rows.push(row);
        maxColumns = Math.max(maxColumns, body.columns);
        maxDigits = Math.max(maxDigits, body.digits);
      }
      if (cards)
        rows.push({
          type: "gap",
          key: `gap:${file.id}`,
          fileId: file.id,
          height: DIFF_FILE_GAP_PX,
        });
    }
    for (const id of this.bodies.keys())
      if (!keep.has(id)) this.bodies.delete(id);
    return {
      rows,
      hunks,
      fileRows,
      maxColumns,
      numberWidth: Math.max(36, maxDigits * 8 + 12),
      ...layoutRows(rows),
    };
  }
}

function buildBody(
  file: UnifiedDiffFileModel,
  message: string | undefined,
  stageable: boolean,
): CachedBody {
  const rows: ReviewRow[] = [];
  let columns = 40;
  let digits = 3;
  if (message)
    rows.push({
      type: "notice",
      key: `notice:${file.id}`,
      fileId: file.id,
      height: 44,
      text: message,
    });
  else
    file.blocks.forEach((block, index) => {
      if (block.kind === "fold") return;
      const prefix = `${file.id}:${index}`;
      if (
        block.kind === "hunk" &&
        block.lines.some(
          (line) => line.kind === "add" || line.kind === "del",
        ) &&
        !block.lines.some((line) => line.kind === "hunk")
      ) {
        rows.push({
          type: "hunk",
          key: `hunk:${prefix}`,
          fileId: file.id,
          height: UNIFIED_HUNK_PX,
          text: hunkRange(block.lines),
        });
      }
      for (let i = 0; i < block.lines.length; i++) {
        const line = block.lines[i];
        if (line.kind === "hunk") {
          rows.push({
            type: "hunk",
            key: `hunk:${prefix}:${i}`,
            fileId: file.id,
            height: UNIFIED_HUNK_PX,
            text: line.text,
          });
        } else {
          const pos =
            line.pos ?? (block.kind === "hunk" ? block.pos : undefined);
          rows.push({
            type: "line",
            key: `line:${prefix}:${line.kind}:${line.oldNumber}:${line.newNumber}`,
            fileId: file.id,
            line,
            height: UNIFIED_LINE_PX,
            stagePos:
              stageable && (line.kind === "add" || line.kind === "del")
                ? pos
                : undefined,
          });
          columns = Math.max(columns, visualColumns(line.text));
          digits = Math.max(
            digits,
            String(line.oldNumber ?? 0).length,
            String(line.newNumber ?? 0).length,
          );
        }
      }
    });
  return {
    blocks: file.blocks,
    message,
    stageable,
    rows,
    columns,
    digits,
  };
}

function hunkRange(lines: readonly UnifiedLine[]): string {
  const old = lines.flatMap((line) =>
    line.oldNumber == null ? [] : [line.oldNumber],
  );
  const next = lines.flatMap((line) =>
    line.newNumber == null ? [] : [line.newNumber],
  );
  return `@@ -${old[0] ?? 0},${old.length} +${next[0] ?? 0},${next.length} @@`;
}

/** Monospace columns, accounting for tab stops and wide source characters. */
export function visualColumns(text: string): number {
  let columns = 0;
  for (const character of text) {
    if (character === "\t") columns += 4 - (columns % 4);
    else if (character.charCodeAt(0) < 128) columns += 1;
    else
      columns +=
        /[\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}\p{Script=Hangul}\p{Extended_Pictographic}]/u.test(
          character,
        )
          ? 2
          : /\p{Mark}/u.test(character)
            ? 0
            : 1;
  }
  return columns;
}
