import { useCallback, useEffect, useState } from "react";
import { AppState } from "react-native";
import { makeCommandId } from "./nativeLink";
import { useConnection } from "./ConnectionProvider";

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

/** The last status read for each working copy, shown at once while it is read again. */
const lastRead = new Map<string, GitStatus>();

/**
 * The branch status behind the commit sheet, read while `enabled`, and a way to
 * run its actions on the desktop. Generating a message or pushing can take
 * longer than a request may, so an action is asked for again under the same ID
 * until the desktop has its answer.
 */
export function useProjectGit(
  projectId: string,
  enabled: boolean,
  worktreePath?: string,
  /** The composer's harness, which writes commit messages and pull requests. */
  harness?: string,
) {
  const { command } = useConnection();
  const scope = worktreePath ? { projectId, worktreePath } : { projectId };
  const scopeKey = `${projectId}\n${worktreePath ?? ""}`;
  const [status, setStatus] = useState<GitStatus | null>(() => lastRead.get(scopeKey) ?? null);
  const [error, setError] = useState<string | null>(null);
  const [nonce, setNonce] = useState(0);
  const [shown, setShown] = useState(scopeKey);
  if (shown !== scopeKey) {
    setShown(scopeKey);
    setStatus(lastRead.get(scopeKey) ?? null);
    setError(null);
  }

  useEffect(() => {
    if (!enabled) return;
    let disposed = false;
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      if (AppState.currentState === "active") {
        try {
          const value = await command<GitStatus>("project.git", {
            projectId,
            ...(worktreePath ? { worktreePath } : {}),
          });
          lastRead.set(`${projectId}\n${worktreePath ?? ""}`, value);
          if (!disposed) {
            setStatus(value);
            setError(null);
          }
        } catch (reason) {
          if (!disposed) setError(reason instanceof Error ? reason.message : String(reason));
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

  return { status, error, reload, run };
}
