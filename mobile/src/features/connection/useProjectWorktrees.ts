import { useEffect, useState } from "react";
import type { Worktree } from "@/features/workspace/types";
import { useConnection } from "./ConnectionProvider";
/** The last list read for each project, shown at once while it is read again. */
const lastRead = new Map<string, Worktree[]>();
/**
 * The project's other worktrees, fetched once per screen for the workspace
 * menu. `loaded` once there is a list to go by, from this read or the last.
 * Read only once `enabled`; until then the last list stands.
 */
export function useProjectWorktrees(projectId: string, enabled = true) {
  const { command } = useConnection();
  const [worktrees, setWorktrees] = useState(() => lastRead.get(projectId));
  useEffect(() => {
    if (!enabled) return;
    let disposed = false;
    command<{ worktrees: Worktree[] }>("project.worktrees", { projectId })
      .then((value) => {
        lastRead.set(projectId, value.worktrees);
        if (!disposed) setWorktrees(value.worktrees);
      })
      // An older desktop doesn't answer this; the menu then offers no existing ones.
      .catch(() => {
        if (!disposed) setWorktrees((current) => current ?? []);
      });
    return () => {
      disposed = true;
    };
  }, [projectId, command, enabled]);
  return { worktrees: worktrees ?? [], loaded: worktrees !== undefined };
}
