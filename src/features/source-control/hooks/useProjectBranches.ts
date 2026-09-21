import { useCallback, useSyncExternalStore } from "react";
import { gitBranches, subscribeGitChanged, type GitBranches } from "../../../platform/tauri/fs";

export type ProjectBranchesState = {
  branches: GitBranches | null;
  /** First lookup for this cwd has finished, repo or not. */
  settled: boolean;
};

const PENDING: ProjectBranchesState = { branches: null, settled: false };

type Entry = {
  cwd: string;
  state: ProjectBranchesState;
  listeners: Set<() => void>;
  inFlight: boolean;
  unsubscribeGit: (() => void) | null;
  onResume: (() => void) | null;
};

const entries = new Map<string, Entry>();

function branchesEqual(a: GitBranches | null, b: GitBranches | null): boolean {
  if (a === b) return true;
  if (!a || !b) return false;
  if (
    a.current !== b.current ||
    a.detached !== b.detached ||
    a.branches.length !== b.branches.length
  ) {
    return false;
  }
  return a.branches.every((branch, index) => {
    const other = b.branches[index];
    return (
      other != null &&
      branch.name === other.name &&
      branch.current === other.current &&
      branch.remote === other.remote
    );
  });
}

function entryFor(cwd: string): Entry {
  const existing = entries.get(cwd);
  if (existing) return existing;
  const entry: Entry = {
    cwd,
    state: PENDING,
    listeners: new Set(),
    inFlight: false,
    unsubscribeGit: null,
    onResume: null,
  };
  entries.set(cwd, entry);
  return entry;
}

function publish(entry: Entry, branches: GitBranches | null) {
  // `settled` still has to flip on a lookup that found nothing, so an
  // unchanged `null` is only a no-op once the first one has landed.
  if (entry.state.settled && branchesEqual(entry.state.branches, branches)) {
    return;
  }
  entry.state = { branches, settled: true };
  for (const listener of entry.listeners) listener();
}

async function load(entry: Entry, force = false) {
  if (entry.inFlight || (!force && document.hidden)) return;
  entry.inFlight = true;
  try {
    publish(entry, await gitBranches(entry.cwd));
  } catch {
    publish(entry, null);
  } finally {
    entry.inFlight = false;
  }
}

function start(entry: Entry) {
  if (entry.onResume) return;
  void load(entry, true);
  entry.onResume = () => {
    if (!document.hidden) void load(entry, true);
  };
  window.addEventListener("focus", entry.onResume);
  document.addEventListener("visibilitychange", entry.onResume);
  entry.unsubscribeGit = subscribeGitChanged(entry.onResume);
}

function stop(entry: Entry) {
  if (entry.onResume) {
    window.removeEventListener("focus", entry.onResume);
    document.removeEventListener("visibilitychange", entry.onResume);
  }
  entry.unsubscribeGit?.();
  entry.onResume = null;
  entry.unsubscribeGit = null;
}

/** Branch list plus whether git has answered yet, for callers that must not
 *  confuse "still looking" with "not a repo". */
export function useProjectBranchesState(
  cwd: string,
  enabled: boolean,
): ProjectBranchesState {
  const active = enabled && Boolean(cwd) && cwd !== "~";
  const subscribe = useCallback(
    (listener: () => void) => {
      if (!active) return () => undefined;
      const entry = entryFor(cwd);
      entry.listeners.add(listener);
      if (entry.listeners.size === 1) start(entry);
      return () => {
        entry.listeners.delete(listener);
        if (entry.listeners.size === 0) stop(entry);
      };
    },
    [active, cwd],
  );
  const getSnapshot = useCallback(() => {
    return active ? entryFor(cwd).state : PENDING;
  }, [active, cwd]);

  return useSyncExternalStore(subscribe, getSnapshot, getSnapshot);
}

export function useProjectBranches(
  cwd: string,
  enabled: boolean,
): GitBranches | null {
  return useProjectBranchesState(cwd, enabled).branches;
}
