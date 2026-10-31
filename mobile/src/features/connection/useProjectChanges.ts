import { useCallback, useEffect, useMemo, useState } from "react";
import { AppState } from "react-native";
import type { ChangedFile, ChangeSummary } from "@/features/workspace/types";
import { useConnection } from "./ConnectionProvider";
type Changes = {
  branch: string;
  /** Staged first, then the rest; with a limit, only the first so many. */
  files: ChangedFile[];
  worktreePath?: string;
  hash?: string;
  /** Every change counted; absent from an older desktop, which sends them all. */
  total?: number;
  stagedTotal?: number;
  additions?: number;
  deletions?: number;
};
/** A desktop that hashes its replies says only this when the phone has the latest. */
type Reply = Changes | { unchanged: true; hash: string; worktreePath?: string };
/** The last changes read for each working copy, so a screen moved to shows
 *  them at once while it reads them again. */
const lastRead = new Map<string, Changes>();
const keyFor = (projectId: string, worktreePath: string | undefined, limit?: number) =>
  `${projectId}\n${worktreePath ?? ""}\n${limit === 0 ? "summary" : "list"}`;

/** What a reply counts, or what its files add up to from an older desktop. */
export function summarize(changes: Changes | null): ChangeSummary {
  if (!changes) return { files: 0, staged: 0, additions: 0, deletions: 0 };
  if (changes.total !== undefined)
    return {
      files: changes.total,
      staged: changes.stagedTotal ?? 0,
      additions: changes.additions ?? 0,
      deletions: changes.deletions ?? 0,
    };
  const { files } = changes;
  return {
    files: files.length,
    staged: files.filter((file) => file.staged && !file.unstaged).length,
    additions: files.reduce((sum, file) => sum + file.additions, 0),
    deletions: files.reduce((sum, file) => sum + file.deletions, 0),
  };
}

/**
 * The project checkout's changes, or a worktree's when given its path. With
 * `limit`, only that many files are read, staged first, though the summary
 * counts them all; at 0, only the summary, for screens that just count them.
 */
export function useProjectChanges(
  projectId: string,
  enabled: boolean,
  worktreePath?: string,
  limit?: number,
) {
  const { command } = useConnection();
  const [result, setResult] = useState<Changes | null>(
    () => lastRead.get(keyFor(projectId, worktreePath, limit)) ?? null,
  );
  const [error, setError] = useState<string | null>(null);
  // Bumped to read again at once, say after a commit.
  const [nonce, setNonce] = useState(0);
  // Another working copy's changes aren't these; clear them rather than show them meanwhile.
  const [shown, setShown] = useState(worktreePath);
  if (shown !== worktreePath) {
    setShown(worktreePath);
    setResult(lastRead.get(keyFor(projectId, worktreePath, limit)) ?? null);
    setError(null);
  }
  useEffect(() => {
    if (!enabled) return;
    let disposed = false;
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      if (AppState.currentState === "active") {
        try {
          const key = keyFor(projectId, worktreePath, limit);
          const before = lastRead.get(key);
          // With the hash of what is held, the desktop answers in a few bytes
          // when nothing changed, rather than resending every file.
          const reply = await command<Reply>("project.changes", {
            projectId,
            ...(worktreePath ? { worktreePath } : {}),
            ...(before?.hash ? { since: before.hash } : {}),
            ...(limit !== undefined ? { limit } : {}),
          });
          // A desktop that predates worktrees answers with the checkout's
          // changes; say so rather than show them as the worktree's.
          if (reply.worktreePath !== worktreePath)
            throw new Error("Update or reload Monocode on your desktop to see this worktree's changes.");
          // Unchanged, the screen showing them needn't render again. Without a
          // hash, an older desktop's reply is compared whole.
          const value = "unchanged" in reply ? before : reply;
          const same =
            !!before &&
            (value === before ||
              (value?.hash
                ? value.hash === before.hash
                : JSON.stringify(before) === JSON.stringify(value)));
          if (!same && value) lastRead.set(key, value);
          if (!disposed) {
            setResult((same ? before : value) ?? null);
            setError(null);
          }
        } catch (reason) {
          if (!disposed)
            setError(reason instanceof Error ? reason.message : String(reason));
        }
      }
      if (!disposed) timer = setTimeout(() => void poll(), 10000);
    };
    void poll();
    return () => {
      disposed = true;
      clearTimeout(timer);
    };
  }, [projectId, enabled, worktreePath, command, nonce, limit]);
  const reload = useCallback(() => setNonce((n) => n + 1), []);
  const summary = useMemo(() => summarize(result), [result]);
  return { result, error, reload, summary };
}
