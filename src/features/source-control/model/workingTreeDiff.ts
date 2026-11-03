import type { GitChangedFile, GitFileDiffKind } from "../../../platform/tauri/fs";

export type WorkingTreeDiffEntry = {
  id: string;
  kind: GitFileDiffKind;
  file: GitChangedFile;
};

export function workingTreeDiffEntryId(
  kind: GitFileDiffKind,
  relative: string,
): string {
  return `${kind}:${relative}`;
}

/**
 * Staged entries come first, matching the source-control sidebar. `scope`
 * keeps only that side, for a review opened from one sidebar section.
 */
export function workingTreeDiffEntries(
  files: readonly GitChangedFile[],
  scope?: GitFileDiffKind,
): WorkingTreeDiffEntry[] {
  const entries: WorkingTreeDiffEntry[] = [];
  const kinds = scope ? [scope] : (["staged", "unstaged"] as const);
  for (const kind of kinds) {
    for (const file of files) {
      if (kind === "staged" ? file.staged : file.unstaged) {
        entries.push({
          id: workingTreeDiffEntryId(kind, file.relative),
          kind,
          file,
        });
      }
    }
  }
  return entries;
}

export function workingTreeDiffEntryLabel(entry: WorkingTreeDiffEntry): string {
  if (!entry.file.staged || !entry.file.unstaged) return entry.file.relative;
  return `${entry.file.relative} (${entry.kind === "staged" ? "Staged" : "Unstaged"})`;
}

export function workingTreeDiffFocusId(
  entries: readonly WorkingTreeDiffEntry[],
  focusPath: string | undefined,
  focusKind?: GitFileDiffKind,
): string | undefined {
  if (!focusPath) return undefined;
  return entries.find(
    (entry) =>
      (!focusKind || entry.kind === focusKind) &&
      (entry.file.path === focusPath || entry.file.relative === focusPath),
  )?.id;
}

export function prioritizeWorkingTreeDiffEntries(
  entries: readonly WorkingTreeDiffEntry[],
  focusPath: string | undefined,
  focusKind?: GitFileDiffKind,
): WorkingTreeDiffEntry[] {
  const focusId = workingTreeDiffFocusId(entries, focusPath, focusKind);
  if (!focusId) return [...entries];
  const focused = entries.find((entry) => entry.id === focusId);
  if (!focused) return [...entries];
  return [focused, ...entries.filter((entry) => entry !== focused)];
}
