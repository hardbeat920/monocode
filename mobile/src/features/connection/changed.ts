import type { ChangedFile, DiffLine } from "@/features/workspace/types";

const sameLine = (a: DiffLine, b: DiffLine) =>
  a.kind === b.kind &&
  a.oldNumber === b.oldNumber &&
  a.newNumber === b.newNumber &&
  a.text === b.text;

const sameStrings = (a?: string[], b?: string[]) =>
  a === b || (!!a && !!b && a.length === b.length && a.every((value, i) => value === b[i]));

/**
 * Whether two reads of a file are the same. Compares field by field, lines
 * last; no strings are built, and the first difference ends it.
 */
export function sameFile(a: ChangedFile, b: ChangedFile): boolean {
  if (
    a.relative !== b.relative ||
    a.status !== b.status ||
    a.additions !== b.additions ||
    a.deletions !== b.deletions ||
    a.staged !== b.staged ||
    a.unstaged !== b.unstaged ||
    a.lines.length !== b.lines.length ||
    !sameStrings(a.palette, b.palette)
  )
    return false;
  for (let i = 0; i < a.lines.length; i++) if (!sameLine(a.lines[i], b.lines[i])) return false;
  return true;
}

export type Changes = { branch: string; files: ChangedFile[]; worktreePath?: string };

/**
 * The new read, keeping each file that did not change as it was and the whole
 * read when nothing did, so the screens showing it render only for a change.
 */
export function mergeChanges(held: Changes | null, next: Changes): Changes {
  if (!held) return next;
  const before = new Map(held.files.map((file) => [file.relative, file]));
  const files = next.files.map((file) => {
    const old = before.get(file.relative);
    return old && sameFile(old, file) ? old : file;
  });
  const same =
    held.branch === next.branch &&
    held.worktreePath === next.worktreePath &&
    files.length === held.files.length &&
    files.every((file, i) => file === held.files[i]);
  return same ? held : { ...next, files };
}

/** Whether two flat records hold the same values (one level deep for `pr`). */
export function sameRecord<T extends object>(a: T, b: T): boolean {
  const left = a as Record<string, unknown>;
  const right = b as Record<string, unknown>;
  const keys = Object.keys(left);
  if (keys.length !== Object.keys(right).length) return false;
  return keys.every((key) => {
    const x = left[key];
    const y = right[key];
    if (x === y) return true;
    return !!x && !!y && typeof x === "object" && typeof y === "object" && sameRecord(x, y);
  });
}
