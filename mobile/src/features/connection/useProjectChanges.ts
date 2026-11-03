import { useCallback, useEffect, useState } from "react";
import { AppState } from "react-native";
import type { ChangedFile } from "@/features/workspace/types";
import { useConnection } from "./ConnectionProvider";
type Changes = { branch: string; files: ChangedFile[]; worktreePath?: string };
/** The last changes read for each working copy, so a screen moved to shows
 *  them at once while it reads them again. */
const lastRead = new Map<string, Changes>();
const keyFor = (projectId: string, worktreePath?: string) =>
  `${projectId}\n${worktreePath ?? ""}`;
/** The project checkout's changes, or a worktree's when given its path. */
export function useProjectChanges(projectId: string, enabled: boolean, worktreePath?: string) {
  const { command } = useConnection();
  const [result, setResult] = useState<Changes | null>(
    () => lastRead.get(keyFor(projectId, worktreePath)) ?? null,
  );
  const [error, setError] = useState<string | null>(null);
  // Bumped to read again at once, say after a commit.
  const [nonce, setNonce] = useState(0);
  // Another working copy's changes aren't these; clear them rather than show them meanwhile.
  const [shown, setShown] = useState(worktreePath);
  if (shown !== worktreePath) {
    setShown(worktreePath);
    setResult(lastRead.get(keyFor(projectId, worktreePath)) ?? null);
    setError(null);
  }
  useEffect(() => {
    if (!enabled) return;
    let disposed = false;
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      if (AppState.currentState === "active") {
        try {
          const value = await command<Changes>("project.changes", {
            projectId,
            ...(worktreePath ? { worktreePath } : {}),
          });
          // A desktop that predates worktrees answers with the checkout's
          // changes; say so rather than show them as the worktree's.
          if (value.worktreePath !== worktreePath)
            throw new Error("Update or reload Monocode on your desktop to see this worktree's changes.");
          const key = keyFor(projectId, worktreePath);
          // Unchanged, the screen showing them needn't render again.
          const before = lastRead.get(key);
          const same = !!before && JSON.stringify(before) === JSON.stringify(value);
          if (!same) lastRead.set(key, value);
          if (!disposed) {
            setResult(same ? before : value);
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
  }, [projectId, enabled, worktreePath, command, nonce]);
  const reload = useCallback(() => setNonce((n) => n + 1), []);
  return { result, error, reload };
}
