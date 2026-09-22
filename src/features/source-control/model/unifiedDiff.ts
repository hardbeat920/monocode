import { Chunk } from "@codemirror/merge";
import { Text } from "@codemirror/state";

const DIFF_CONFIG = { scanLimit: 5_000, timeout: 100 };

export const UNIFIED_CONTEXT_DEFAULT = 3;
export const UNIFIED_FOLD_STEP = 20;

export type UnifiedLineKind = "add" | "del" | "context" | "hunk";

export type UnifiedLine = {
  kind: UnifiedLineKind;
  text: string;
  oldNumber: number | null;
  newNumber: number | null;
  /** Document position in the current file, used for hunk stage/revert. */
  pos?: number;
};

export type UnifiedBlock =
  | { kind: "hunk"; lines: UnifiedLine[]; pos?: number }
  | { kind: "fold"; id: string; lines: UnifiedLine[] };

export type FoldReveal = { start: number; end: number };

export type UnifiedFileDiff = {
  additions: number;
  deletions: number;
  lines: UnifiedLine[];
  blocks: UnifiedBlock[];
};

export function buildUnifiedFile(
  original: string,
  current: string,
  context = UNIFIED_CONTEXT_DEFAULT,
): UnifiedFileDiff {
  const lines = unifiedLinesFromTexts(original, current);
  let additions = 0;
  let deletions = 0;
  for (const line of lines) {
    if (line.kind === "add") additions += 1;
    else if (line.kind === "del") deletions += 1;
  }
  return {
    additions,
    deletions,
    lines,
    blocks: foldUnifiedLines(lines, context),
  };
}

export function blocksFromLines(
  lines: readonly UnifiedLine[],
  context = UNIFIED_CONTEXT_DEFAULT,
): UnifiedBlock[] {
  const copy = lines.map((line) => ({ ...line }));
  return foldUnifiedLines(copy, context);
}

export function foldUnifiedLines(
  lines: readonly UnifiedLine[],
  context = UNIFIED_CONTEXT_DEFAULT,
): UnifiedBlock[] {
  if (lines.length === 0) return [];
  const visible = visibleContext(lines, context);
  const blocks: UnifiedBlock[] = [];
  let index = 0;
  let foldId = 0;
  while (index < lines.length) {
    const line = lines[index];
    if (line.kind === "context" && !visible[index]) {
      const start = index;
      while (
        index < lines.length &&
        lines[index].kind === "context" &&
        !visible[index]
      ) {
        index += 1;
      }
      blocks.push({
        kind: "fold",
        id: `fold-${foldId}`,
        lines: lines.slice(start, index),
      });
      foldId += 1;
      continue;
    }
    const start = index;
    let pos: number | undefined;
    while (index < lines.length) {
      const next = lines[index];
      const hiddenContext = next.kind === "context" && !visible[index];
      if (hiddenContext) break;
      if (pos == null && next.pos != null) pos = next.pos;
      index += 1;
    }
    blocks.push({
      kind: "hunk",
      lines: lines.slice(start, index),
      pos,
    });
  }
  return blocks;
}

export function revealedFold(
  total: number,
  reveal: FoldReveal | undefined,
): { head: number; tail: number; hidden: number } {
  const start = Math.max(0, Math.min(reveal?.start ?? 0, total));
  const remaining = total - start;
  const end = Math.max(0, Math.min(reveal?.end ?? 0, remaining));
  return {
    head: start,
    tail: end,
    hidden: total - start - end,
  };
}

export function expandFold(
  reveal: FoldReveal | undefined,
  total: number,
  direction: "up" | "down" | "all",
  step = UNIFIED_FOLD_STEP,
): FoldReveal {
  if (direction === "all" || total <= 0) return { start: total, end: 0 };
  const current = reveal ?? { start: 0, end: 0 };
  if (direction === "down") {
    return { start: current.start + step, end: current.end };
  }
  return { start: current.start, end: current.end + step };
}

function unifiedLinesFromTexts(original: string, current: string): UnifiedLine[] {
  const oldDoc = textFromString(original);
  const newDoc = textFromString(current);
  if (oldDoc.eq(newDoc)) return contextLines(newDoc, oldDoc, 0, newDoc.length);
  const chunks = Chunk.build(oldDoc, newDoc, DIFF_CONFIG);
  if (chunks.length === 0) return contextLines(newDoc, oldDoc, 0, newDoc.length);

  const lines: UnifiedLine[] = [];
  let oldPos = 0;
  let newPos = 0;
  for (const chunk of chunks) {
    if (newPos < chunk.fromB) {
      lines.push(
        ...contextLines(newDoc, oldDoc, newPos, chunk.fromB, oldPos),
      );
    }
    const deleted = linesInRange(oldDoc, chunk.fromA, chunk.toA);
    const inserted = linesInRange(newDoc, chunk.fromB, chunk.toB);
    for (const line of deleted) {
      lines.push({
        kind: "del",
        text: line.text,
        oldNumber: line.number,
        newNumber: null,
        pos: chunk.fromB,
      });
    }
    for (const line of inserted) {
      lines.push({
        kind: "add",
        text: line.text,
        oldNumber: null,
        newNumber: line.number,
        pos: chunk.fromB,
      });
    }
    oldPos = chunk.toA;
    newPos = chunk.toB;
  }
  if (newPos < newDoc.length) {
    lines.push(...contextLines(newDoc, oldDoc, newPos, newDoc.length, oldPos));
  }
  return lines;
}

function contextLines(
  newDoc: Text,
  oldDoc: Text,
  fromB: number,
  toB: number,
  fromA = fromB,
): UnifiedLine[] {
  const inserted = linesInRange(newDoc, fromB, toB);
  if (inserted.length === 0) return [];
  const deleted = linesInRange(oldDoc, fromA, fromA + (toB - fromB));
  return inserted.map((line, index) => ({
    kind: "context" as const,
    text: line.text,
    oldNumber: deleted[index]?.number ?? line.number,
    newNumber: line.number,
  }));
}

function linesInRange(
  doc: Text,
  from: number,
  to: number,
): { text: string; number: number }[] {
  if (from >= to || doc.length === 0) return [];
  const end = Math.min(Math.max(from, to - 1), Math.max(0, doc.length - 1));
  const startLine = doc.lineAt(Math.min(from, doc.length));
  const endLine = doc.lineAt(end);
  const out: { text: string; number: number }[] = [];
  for (let number = startLine.number; number <= endLine.number; number += 1) {
    const line = doc.line(number);
    out.push({ text: line.text, number });
  }
  return out;
}

function visibleContext(
  lines: readonly UnifiedLine[],
  context: number,
): boolean[] {
  const visible = lines.map((line) => line.kind !== "context");
  if (context <= 0) return visible;
  for (let index = 0; index < lines.length; index += 1) {
    if (lines[index].kind === "context" || lines[index].kind === "hunk") {
      continue;
    }
    const from = Math.max(0, index - context);
    const to = Math.min(lines.length - 1, index + context);
    for (let inner = from; inner <= to; inner += 1) {
      if (lines[inner].kind === "context") visible[inner] = true;
    }
  }
  return visible;
}

function textFromString(value: string): Text {
  return Text.of(value.split("\n"));
}
