import { useCallback, useSyncExternalStore } from "react";
import { subscribeGitChanged } from "../lib/fs";
import { pathKey } from "../lib/paths";
import { listWorktrees, type Worktrees } from "../lib/worktrees";

type Snapshot = { data?: Worktrees; error?: string };
type Entry = {
  cwd: string;
  snapshot: Snapshot;
  listeners: Set<() => void>;
  inFlight?: Promise<boolean>;
  invalidated: boolean;
  stop?: () => void;
};

const EMPTY: Snapshot = {};
const entries = new Map<string, Entry>();

function entryFor(cwd: string): Entry {
  const key = pathKey(cwd);
  let entry = entries.get(key);
  if (!entry) {
    entry = {
      cwd,
      snapshot: EMPTY,
      listeners: new Set(),
      invalidated: false,
    };
    entries.set(key, entry);
  }
  return entry;
}

function publish(entry: Entry, snapshot: Snapshot) {
  entry.snapshot = snapshot;
  for (const listener of entry.listeners) listener();
}

function load(entry: Entry, invalidated = false): Promise<boolean> {
  if (entry.inFlight) {
    // A Git mutation during a read needs a follow-up; repeated focus/open
    // events can share the request already in progress.
    entry.invalidated ||= invalidated;
    return entry.inFlight;
  }
  entry.inFlight = (async () => {
    let loaded = false;
    try {
      const data = await listWorktrees(entry.cwd);
      publish(entry, { data });
      loaded = true;
    } catch (error) {
      // A background failure must not replace a usable list with a loading or
      // error screen. The caller can still surface the error beside the list.
      publish(entry, { ...entry.snapshot, error: String(error) });
    }
    entry.inFlight = undefined;
    if (entry.invalidated) {
      entry.invalidated = false;
      return load(entry);
    }
    return loaded;
  })();
  return entry.inFlight;
}

function start(entry: Entry) {
  void load(entry);
  const resume = () => {
    if (!document.hidden) void load(entry);
  };
  const unsubscribe = subscribeGitChanged(() => {
    if (!document.hidden) void load(entry, true);
  });
  window.addEventListener("focus", resume);
  document.addEventListener("visibilitychange", resume);
  entry.stop = () => {
    unsubscribe();
    window.removeEventListener("focus", resume);
    document.removeEventListener("visibilitychange", resume);
  };
}

/** Share cached working copies between settings and composers. Revalidation
 * never clears the last successful result, including across unmounts. */
export function useProjectWorktrees(cwd: string, enabled = true) {
  const active = enabled && !!cwd && cwd !== "~";
  const subscribe = useCallback(
    (listener: () => void) => {
      if (!active) return () => {};
      const entry = entryFor(cwd);
      entry.listeners.add(listener);
      if (entry.listeners.size === 1) start(entry);
      return () => {
        entry.listeners.delete(listener);
        if (entry.listeners.size === 0) {
          entry.stop?.();
          entry.stop = undefined;
        }
      };
    },
    [active, cwd],
  );
  const getSnapshot = useCallback(
    () => (active ? entryFor(cwd).snapshot : EMPTY),
    [active, cwd],
  );
  const refresh = useCallback(() => {
    return active ? load(entryFor(cwd), true) : Promise.resolve(false);
  }, [active, cwd]);
  return {
    ...useSyncExternalStore(subscribe, getSnapshot, getSnapshot),
    refresh,
  };
}
