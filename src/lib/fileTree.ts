import { listDir, type FsEntry } from "./fs";
import { pathSegments } from "./fileName";
import { joinPath, parentPath } from "./paths";

const expandedByProject = new Map<string, Set<string>>();
const selectedByProject = new Map<string, string | null>();
const dirs = new Map<string, FsEntry[]>();
const listeners = new Set<() => void>();

const REFRESH_MS = 150;
let refreshTimer: ReturnType<typeof setTimeout> | null = null;
let refreshing = false;
let refreshAgain = false;

export function loadExpanded(cwd: string): Set<string> {
  const saved = expandedByProject.get(cwd);
  return saved ? new Set(saved) : new Set([cwd]);
}

export function saveExpanded(cwd: string, expanded: Set<string>) {
  expandedByProject.set(cwd, new Set(expanded));
}

export function loadSelected(cwd: string): string | null {
  return selectedByProject.get(cwd) ?? null;
}

export function saveSelected(cwd: string, path: string | null) {
  selectedByProject.set(cwd, path);
}

/** Cached `listDir` — same path stays instant when the tree remounts. */
export function peekDir(path: string): FsEntry[] | null {
  return dirs.get(path) ?? null;
}

export function listCachedDir(path: string): Promise<FsEntry[]> {
  const hit = dirs.get(path);
  if (hit) return Promise.resolve(hit);
  return listDir(path).then((entries) => {
    dirs.set(path, entries);
    return entries;
  });
}

export function refreshDir(path: string): Promise<FsEntry[]> {
  dirs.delete(path);
  return listCachedDir(path);
}

export function forgetDir(path: string) {
  for (const key of [...dirs.keys()]) {
    if (key === path || key.startsWith(`${path}/`)) dirs.delete(key);
  }
}

/** Re-list every cached folder. Agent writes and window focus use this. */
export async function refreshCachedDirs(): Promise<void> {
  const paths = [...dirs.keys()];
  if (paths.length === 0) return;
  await Promise.all(
    paths.map((path) =>
      refreshDir(path).catch(() => {
        forgetDir(path);
      }),
    ),
  );
}

export function subscribeDirsChanged(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** Reload the explorer cache after an agent/shell write (debounced). */
export function notifyDirsChanged() {
  if (typeof document !== "undefined" && document.hidden) return;
  scheduleRefresh();
}

function scheduleRefresh() {
  if (refreshTimer != null) return;
  refreshTimer = setTimeout(() => {
    refreshTimer = null;
    void runRefresh();
  }, REFRESH_MS);
}

async function runRefresh() {
  if (refreshing) {
    refreshAgain = true;
    return;
  }
  refreshing = true;
  try {
    await refreshCachedDirs();
    for (const listener of listeners) listener();
  } finally {
    refreshing = false;
    if (refreshAgain) {
      refreshAgain = false;
      scheduleRefresh();
    }
  }
}

/** Folder to create into, given the explorer selection. */
export function createParentOf(cwd: string, selectedPath: string | null): string {
  if (!selectedPath || selectedPath === cwd) return cwd;
  const parent = parentPath(selectedPath);
  const entry = peekDir(parent)?.find((e) => e.path === selectedPath);
  if (entry?.isDir) return selectedPath;
  if (entry && !entry.isDir) return parent;
  if (peekDir(selectedPath)) return selectedPath;
  return parent;
}

/** Directories whose children change when creating `name` under `parent`. */
export function dirsTouchedByCreate(parent: string, name: string): string[] {
  const segments = pathSegments(name);
  const out = [parent];
  let cur = parent;
  for (let i = 0; i < segments.length - 1; i++) {
    cur = joinPath(cur, segments[i]);
    out.push(cur);
  }
  return out;
}

export function dirsTouchedByMove(from: string, to: string): string[] {
  const fromParent = parentPath(from);
  const toParent = parentPath(to);
  return fromParent === toParent ? [fromParent] : [fromParent, toParent];
}
