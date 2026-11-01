import { useCallback, useMemo } from "react";
import { makeCommandId } from "./nativeLink";
import { useCommand } from "./ConnectionProvider";
import { sameRecord } from "./changed";
import { polledResource } from "./polledResource";
import { usePolled } from "./usePolled";

/** How the working copy's branch stands, for the commit sheet's actions. */
export type GitStatus = {
  branch: string;
  files: number;
  remote: boolean;
  upstream: boolean;
  defaultBranch: string;
  ahead: number;
  behind: number;
  aheadOfDefault: number;
  /** Whether the desktop can write a commit message here; not for remote projects. */
  canGenerate: boolean;
  /** The branch's open pull request. */
  pr: { number: number; url: string } | null;
};

export type GitAction =
  | { action: "stage" | "unstage"; paths: string[] }
  | { action: "stageAll" | "unstageAll" }
  | { action: "generate" }
  | { action: "commit"; message: string }
  | { action: "sync" }
  | { action: "pr" };

const resourceFor = (projectId: string, worktreePath?: string) =>
  polledResource<GitStatus>(`git\n${projectId}\n${worktreePath ?? ""}`, {
    merge: (held, next) => (held && sameRecord(held, next) ? held : next),
  });

/**
 * The branch status behind the commit sheet, read while `enabled` and focused,
 * and a way to run its actions on the desktop. Generating a message or pushing
 * can take longer than a request may, so an action is asked for again under
 * the same ID until the desktop has its answer.
 */
export function useProjectGit(
  projectId: string,
  enabled: boolean,
  worktreePath?: string,
  /** The composer's harness, which writes commit messages and pull requests. */
  harness?: string,
) {
  const command = useCommand();
  const scope = worktreePath ? { projectId, worktreePath } : { projectId };
  const resource = useMemo(() => resourceFor(projectId, worktreePath), [projectId, worktreePath]);
  const fetch = useCallback(
    () =>
      command<GitStatus>("project.git", {
        projectId,
        ...(worktreePath ? { worktreePath } : {}),
      }),
    [command, projectId, worktreePath],
  );
  const { value: status, error } = usePolled(resource, fetch, enabled);

  const run = async (action: GitAction) => {
    const id = makeCommandId();
    const params = { ...scope, ...(harness ? { harness } : {}), ...action };
    for (;;) {
      const result = await command<{ pending?: boolean; message?: string; url?: string }>(
        "project.gitAction",
        params,
        id,
      );
      if (!result.pending) return result;
    }
  };

  return { status, error, reload: resource.reload, run };
}
