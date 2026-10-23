import { useSyncExternalStore } from "react";
import { pathKey } from "../../../shared/lib/paths";

/** The working copy a project's sidebar is narrowed to. New sessions in that
 * project start there too. Kept for this run only: the app reopens on each
 * project's default workspace. */
export type WorktreeFocus = {
  path: string;
  branch: string | null;
  /** A CoW working copy belongs to this session; navigation must open it. */
  cowId?: string;
  sessionId?: string;
};

const listeners = new Set<() => void>();
let focuses: Record<string, WorktreeFocus> = {};

export function worktreeFocus(project: string): WorktreeFocus | undefined {
  return focuses[pathKey(project)];
}

export function setWorktreeFocus(
  project: string,
  focus: WorktreeFocus | undefined,
) {
  const next = { ...focuses };
  if (focus) next[pathKey(project)] = focus;
  else delete next[pathKey(project)];
  focuses = next;
  for (const listener of listeners) listener();
}

export function useWorktreeFocus(project: string): WorktreeFocus | undefined {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => worktreeFocus(project),
  );
}

export function inWorktreeFocus(
  session: { cwd: string; worktreeCwd?: string },
  focus: WorktreeFocus | undefined,
): boolean {
  return (
    !focus ||
    pathKey(session.worktreeCwd || session.cwd) === pathKey(focus.path)
  );
}
