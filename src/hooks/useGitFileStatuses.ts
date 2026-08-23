import { useCallback, useSyncExternalStore } from "react";
import {
  gitDiffIndex,
  subscribeGitChanged,
  type GitDiffIndex,
} from "../lib/fs";
import { subscribeDirsChanged } from "../lib/fileTree";
import { parentPath } from "../lib/paths";

export type GitStatusMap = {
  files: Map<string, string>;
  dirs: Map<string, string>;
};

const STATUS_PRIORITY: Record<string, number> = {
  modified: 3,
  deleted: 2,
  added: 1,
  untracked: 1,
};

const EMPTY: GitStatusMap = { files: new Map(), dirs: new Map() };

type Entry = {
  cwd: string;
  data: GitStatusMap;
  listeners: Set<() => void>;
  inFlight: boolean;
  pending: boolean;
  unsubscribeGit: (() => void) | null;
  unsubscribeDirs: (() => void) | null;
  onResume: (() => void) | null;
};

const entries = new Map<string, Entry>();

function entryFor(cwd: string): Entry {
  const existing = entries.get(cwd);
  if (existing) return existing;
  const entry: Entry = {
    cwd,
    data: EMPTY,
    listeners: new Set(),
    inFlight: false,
    pending: false,
    unsubscribeGit: null,
    unsubscribeDirs: null,
    onResume: null,
  };
  entries.set(cwd, entry);
  return entry;
}

function buildStatusMaps(index: GitDiffIndex, cwd: string): GitStatusMap {
  const files = new Map<string, string>();
  const dirs = new Map<string, string>();
  for (const file of index.files) {
    files.set(file.path, file.status);
    const prio = STATUS_PRIORITY[file.status] ?? 0;
    if (prio === 0) continue;
    let dir = parentPath(file.path);
    while (dir.length > cwd.length) {
      const existing = dirs.get(dir);
      const existingPrio = existing ? (STATUS_PRIORITY[existing] ?? 0) : 0;
      if (prio > existingPrio) {
        dirs.set(dir, file.status);
      } else {
        break;
      }
      dir = parentPath(dir);
    }
  }
  return { files, dirs };
}

function sameMaps(
  a: Map<string, string>,
  b: Map<string, string>,
): boolean {
  if (a.size !== b.size) return false;
  for (const [k, v] of b) {
    if (a.get(k) !== v) return false;
  }
  return true;
}

function sameData(a: GitStatusMap, b: GitStatusMap): boolean {
  return sameMaps(a.files, b.files) && sameMaps(a.dirs, b.dirs);
}

function publish(entry: Entry, data: GitStatusMap) {
  if (sameData(entry.data, data)) return;
  entry.data = data;
  for (const listener of entry.listeners) listener();
}

async function load(entry: Entry, force = false) {
  if (entry.inFlight) {
    entry.pending = true;
    return;
  }
  if (!force && document.hidden) return;
  entry.inFlight = true;
  try {
    const index = await gitDiffIndex(entry.cwd);
    publish(entry, buildStatusMaps(index, entry.cwd));
  } catch {
    publish(entry, EMPTY);
  } finally {
    entry.inFlight = false;
    if (entry.pending) {
      entry.pending = false;
      void load(entry, true);
    }
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
  entry.unsubscribeDirs = subscribeDirsChanged(entry.onResume);
}

function stop(entry: Entry) {
  if (entry.onResume) {
    window.removeEventListener("focus", entry.onResume);
    document.removeEventListener("visibilitychange", entry.onResume);
  }
  entry.unsubscribeGit?.();
  entry.unsubscribeDirs?.();
  entry.onResume = null;
  entry.unsubscribeGit = null;
  entry.unsubscribeDirs = null;
}

export function useGitFileStatuses(
  cwd: string,
  enabled: boolean,
): GitStatusMap {
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
    return active ? entryFor(cwd).data : EMPTY;
  }, [active, cwd]);

  return useSyncExternalStore(subscribe, getSnapshot, getSnapshot);
}
