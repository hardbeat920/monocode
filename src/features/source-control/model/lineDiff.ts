import { Change, diff, type DiffConfig } from "@codemirror/merge";

/** Code units reserved for surrogates; line ids skip over them. */
const SURROGATE_START = 0xd800;
const SURROGATE_SIZE = 0x800;
const MAX_LINE_IDS = 0x10000 - SURROGATE_SIZE;

// Budget for the line-level pass. Each "character" is a whole line here, so
// this allows thousands of changed lines before the diff turns imprecise.
const LINE_PASS: DiffConfig = { scanLimit: 5_000, timeout: 200 };

/**
 * Line-granular diff for whole files, shaped as CodeMirror `Change`s so it can
 * back `Chunk.build` through `DiffConfig.override`.
 *
 * CodeMirror's default diff compares characters, so a large file with edits
 * spread through it exceeds `scanLimit` and falls back to one chunk covering
 * most of the file. Diffing lines (the approach git and jsdiff take) keeps the
 * input small: each distinct line becomes one code unit, the encoded strings
 * are diffed, and the result is mapped back to character offsets.
 */
export function lineDiff(a: string, b: string): readonly Change[] {
  // Keep each line's "\n" so offsets are exact prefix sums and a missing
  // final newline shows as a change, like git.
  const linesA = splitKeepingNewlines(a);
  const linesB = splitKeepingNewlines(b);
  const ids = new Map<string, number>();
  const encodedA = encode(linesA, ids);
  const encodedB = encodedA == null ? null : encode(linesB, ids);
  if (encodedA == null || encodedB == null) return diff(a, b, LINE_PASS);

  const offsetsA = offsets(linesA);
  const offsetsB = offsets(linesB);
  return diff(encodedA, encodedB, LINE_PASS).map(
    (change) =>
      new Change(
        offsetsA[change.fromA],
        offsetsA[change.toA],
        offsetsB[change.fromB],
        offsetsB[change.toB],
      ),
  );
}

export const LINE_DIFF_CONFIG: DiffConfig = { override: lineDiff };

function splitKeepingNewlines(text: string): string[] {
  const lines: string[] = [];
  let start = 0;
  for (;;) {
    const end = text.indexOf("\n", start);
    if (end < 0) break;
    lines.push(text.slice(start, end + 1));
    start = end + 1;
  }
  if (start < text.length) lines.push(text.slice(start));
  return lines;
}

/** One code unit per line; null when there are too many distinct lines. */
function encode(lines: readonly string[], ids: Map<string, number>): string | null {
  const units = new Array<number>(lines.length);
  for (let index = 0; index < lines.length; index += 1) {
    let id = ids.get(lines[index]);
    if (id == null) {
      id = ids.size;
      if (id >= MAX_LINE_IDS) return null;
      ids.set(lines[index], id);
    }
    units[index] = id < SURROGATE_START ? id : id + SURROGATE_SIZE;
  }
  let out = "";
  // Chunked to stay under the engine's argument-count limit.
  for (let index = 0; index < units.length; index += 8192) {
    out += String.fromCharCode(...units.slice(index, index + 8192));
  }
  return out;
}

/** `result[i]` is the character offset where line `i` starts. */
function offsets(lines: readonly string[]): number[] {
  const result = new Array<number>(lines.length + 1);
  result[0] = 0;
  for (let index = 0; index < lines.length; index += 1) {
    result[index + 1] = result[index] + lines[index].length;
  }
  return result;
}
