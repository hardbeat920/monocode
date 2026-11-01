import { useCallback, useMemo } from "react";
import { useCommand } from "./ConnectionProvider";
import { mergeChanges, type Changes } from "./changed";
import { polledResource } from "./polledResource";
import { usePolled } from "./usePolled";

const resourceFor = (projectId: string, worktreePath?: string) =>
  polledResource<Changes>(`changes\n${projectId}\n${worktreePath ?? ""}`, {
    merge: mergeChanges,
  });

/**
 * The project checkout's changes, or a worktree's when given its path. Every
 * screen reading the same working copy shares one poller, which runs only
 * while one is up and focused and the app is in the foreground.
 */
export function useProjectChanges(projectId: string, enabled: boolean, worktreePath?: string) {
  const command = useCommand();
  const resource = useMemo(() => resourceFor(projectId, worktreePath), [projectId, worktreePath]);
  const fetch = useCallback(async () => {
    const value = await command<Changes>("project.changes", {
      projectId,
      ...(worktreePath ? { worktreePath } : {}),
    });
    // A desktop that predates worktrees answers with the checkout's
    // changes; say so rather than show them as the worktree's.
    if (value.worktreePath !== worktreePath)
      throw new Error("Update or reload Monocode on your desktop to see this worktree's changes.");
    return value;
  }, [command, projectId, worktreePath]);
  const { value, error } = usePolled(resource, fetch, enabled);
  return { result: value, error, reload: resource.reload };
}
