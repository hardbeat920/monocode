import { listDir, type FsEntry } from "../../../platform/tauri/fs";
import { pathSegments } from "./fileName";
import { joinPath, parentPath, slash } from "../../../shared/lib/paths";

const expandedByProject = new Map<string, Set<string>>();
const selectedByProject = new Map<string, string | null>();
const dirs = new Map<string, FsEntry[]>();
const listeners = new Set<() => void>();

const REFRESH_MS = 500;
let refreshTimer: ReturnType<typeof setTimeout> | null = null;
let refreshing = false;
let refreshAgain = false;
/** Projets à re-lister au prochain tick. `null` = tout (focus, chemin inconnu). */
let pendingScope: Set<string> | null = new Set<string>();

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
export async function refreshCachedDirs(under?: string | null): Promise<void> {
  await refreshScoped(under ? new Set([normalizeScope(under)]) : null);
}

function normalizeScope(path: string): string {
  const normalized = slash(path);
  return normalized.length > 1 ? normalized.replace(/\/+$/, "") : normalized;
}

function inScope(path: string, root: string): boolean {
  if (path === root) return true;
  return path.startsWith(root.endsWith("/") ? root : `${root}/`);
}

function scopedPaths(scope: Set<string> | null): string[] {
  const all = [...dirs.keys()];
  if (!scope || scope.size === 0) return all;
  const roots = [...scope];
  const scoped = all.filter((path) => {
    const key = slash(path);
    return roots.some((root) => inScope(key, root));
  });
  // Un scope qui ne correspond à aucune entrée connue (chemin non normalisé,
  // projet fraîchement ouvert) doit rafraîchir largement, pas rien. Un scope
  // sans correspondance ne doit pas être masqué par un autre qui matche.
  const allMatched = roots.every((root) =>
    all.some((path) => inScope(slash(path), root)),
  );
  return allMatched || all.length === 0 ? scoped : all;
}

async function refreshScoped(scope: Set<string> | null): Promise<void> {
  const paths = scopedPaths(scope);
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
export function notifyDirsChanged(under?: string) {
  if (typeof document !== "undefined" && document.hidden) return;
  if (!under) {
    pendingScope = null;
  } else if (pendingScope !== null) {
    pendingScope.add(normalizeScope(under));
  }
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
  const scope = pendingScope;
  pendingScope = new Set<string>();
  try {
    await refreshScoped(scope);
    for (const listener of listeners) listener();
  } finally {
    refreshing = false;
    const queued = pendingScope === null || pendingScope.size > 0;
    if (refreshAgain || queued) {
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
