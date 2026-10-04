import { listDir, type FsEntry } from "../../../platform/tauri/fs";
import { pathSegments } from "./fileName";
import { joinPath, parentPath } from "../../../shared/lib/paths";

const expandedByProject = new Map<string, Set<string>>();
const selectedByProject = new Map<string, string | null>();
const dirs = new Map<string, FsEntry[]>();
const listeners = new Set<() => void>();
/** Roots with an explorer on screen, counted so two can share a cwd. */
const mountedRoots = new Map<string, number>();

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

/**
 * An expanded set without `path` or anything under it, for when a folder is
 * deleted. Returns the same set when there was nothing to drop.
 */
export function withoutSubtree(
  expanded: Set<string>,
  path: string,
): Set<string> {
  const next = new Set(
    [...expanded].filter((p) => p !== path && !p.startsWith(`${path}/`)),
  );
  return next.size === expanded.size ? expanded : next;
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

/**
 * Roots with an explorer on screen — only their listings stay worth keeping.
 *
 * Counted rather than held in a set: two explorers can share a cwd, and the
 * first one to unmount must not drop the root the other is still showing.
 */
export function registerExplorer(cwd: string) {
  mountedRoots.set(cwd, (mountedRoots.get(cwd) ?? 0) + 1);
}

export function unregisterExplorer(cwd: string) {
  const count = mountedRoots.get(cwd);
  if (count === undefined) return;
  if (count > 1) mountedRoots.set(cwd, count - 1);
  else mountedRoots.delete(cwd);
}

/** Folders the mounted explorers can actually show right now. */
function visibleDirs(): Set<string> {
  const visible = new Set<string>();
  for (const root of mountedRoots.keys()) {
    visible.add(root);
    const expanded = expandedByProject.get(root);
    if (!expanded) continue;
    for (const path of expanded) {
      if (path === root || path.startsWith(`${root}/`)) {
        if (
          path === root ||
          (expanded.has(root) && everyFolderAbove(path, root, expanded))
        ) {
          visible.add(path);
        }
      }
    }
  }
  return visible;
}

/**
 * Collapsing a folder only drops that one path from the expanded set, so its
 * descendants linger there. A descendant is on screen only while the root and
 * every folder between it and the root are expanded.
 */
function everyFolderAbove(
  path: string,
  root: string,
  expanded: Set<string>,
): boolean {
  let end = root.length;
  while (end < path.length) {
    const slash = path.indexOf("/", end + 1);
    if (slash === -1) return true;
    if (!expanded.has(path.slice(0, slash))) return false;
    end = slash;
  }
  return true;
}

/**
 * Re-list what the mounted explorers show, drop the rest.
 *
 * Agent writes and window focus use this. Collapsed subtrees and other projects
 * are evicted; expanding them again re-lists on demand.
 */
export async function refreshCachedDirs(): Promise<void> {
  const visible = visibleDirs();
  for (const path of [...dirs.keys()]) {
    if (!visible.has(path)) dirs.delete(path);
  }
  const paths = [...visible].filter((path) => dirs.has(path));
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
export function createParentOf(
  cwd: string,
  selectedPath: string | null,
): string {
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
