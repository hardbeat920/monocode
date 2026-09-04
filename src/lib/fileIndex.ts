import { listProjectFiles, type ProjectFile } from "./fs";
import { subscribeDirsChanged } from "./fileTree";
import { scorePath, type FuzzyHit } from "./fuzzy";
import { resolveWorkspacePath } from "./paths";
import { looksLikeProject } from "./recents";
import { normalizeEditorPath } from "./search";

const MAX_RECENTS = 30;
const MAX_RESULTS = 80;
const REFRESH_MS = 150;

type Cache = {
  cwd: string;
  files: ProjectFile[];
};

type Listener = () => void;

let cache: Cache | null = null;
let inflight: { cwd: string; promise: Promise<ProjectFile[]> } | null = null;
let lastCwd: string | null = null;
let epoch = 0;
let refreshTimer: ReturnType<typeof setTimeout> | null = null;
let refreshing = false;
let refreshAgain = false;
const listeners = new Set<Listener>();
const recentsByCwd = new Map<string, string[]>();

function normCwd(cwd: string): string {
  return cwd.replace(/\/+$/, "") || "/";
}

function notifyProjectFilesChanged() {
  for (const listener of listeners) listener();
}

export function subscribeProjectFiles(listener: Listener): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function peekProjectFiles(cwd: string): ProjectFile[] | null {
  return cache?.cwd === cwd ? cache.files : null;
}

export function invalidateProjectFiles(cwd?: string) {
  if (cwd && cache?.cwd !== cwd && inflight?.cwd !== cwd) return;
  if (!cwd || cache?.cwd === cwd) cache = null;
  if (!cwd || inflight?.cwd === cwd) {
    inflight = null;
    epoch += 1;
  }
  if (!cwd) {
    lastCwd = null;
    if (refreshTimer != null) {
      clearTimeout(refreshTimer);
      refreshTimer = null;
    }
  }
  notifyProjectFilesChanged();
}

function scheduleIndexRefresh() {
  if (!lastCwd) return;
  if (typeof document !== "undefined" && document.hidden) return;
  if (refreshTimer != null) return;
  refreshTimer = setTimeout(() => {
    refreshTimer = null;
    void runIndexRefresh();
  }, REFRESH_MS);
}

async function runIndexRefresh() {
  if (refreshing) {
    refreshAgain = true;
    return;
  }
  const cwd = lastCwd;
  if (!cwd) return;
  refreshing = true;
  try {
    await loadProjectFiles(cwd, true);
  } catch {
    /* next focus / dir change will retry */
  } finally {
    refreshing = false;
    if (refreshAgain) {
      refreshAgain = false;
      scheduleIndexRefresh();
    }
  }
}

export function rememberOpenedFile(cwd: string, path: string) {
  if (!path) return;
  const key = normCwd(cwd);
  const prev = recentsByCwd.get(key) ?? [];
  recentsByCwd.set(
    key,
    [path, ...prev.filter((item) => item !== path)].slice(0, MAX_RECENTS),
  );
}

export function recentOpenedFiles(cwd: string): string[] {
  return recentsByCwd.get(normCwd(cwd)) ?? [];
}

export function prefetchProjectFiles(cwd: string) {
  if (!looksLikeProject(cwd)) return;
  void loadProjectFiles(cwd);
}

export function loadProjectFiles(
  cwd: string,
  refresh = false,
): Promise<ProjectFile[]> {
  if (!looksLikeProject(cwd)) return Promise.resolve([]);
  lastCwd = cwd;
  if (!refresh && cache?.cwd === cwd) return Promise.resolve(cache.files);
  if (!refresh && inflight?.cwd === cwd) return inflight.promise;

  const id = ++epoch;
  const promise = listProjectFiles(cwd)
    .then((files) => {
      if (id !== epoch) return files;
      cache = { cwd, files };
      notifyProjectFilesChanged();
      return files;
    })
    .finally(() => {
      if (inflight?.promise === promise) inflight = null;
    });
  inflight = { cwd, promise };
  return promise;
}

export type RankedFile = ProjectFile & FuzzyHit;

export function rankProjectFiles(
  files: ProjectFile[],
  query: string,
  recents: string[],
  limit = MAX_RESULTS,
): RankedFile[] {
  const recentRank = new Map(recents.map((path, index) => [path, index]));

  if (!query.trim()) {
    const byPath = new Map(files.map((file) => [file.path, file]));
    const out: RankedFile[] = [];
    const seen = new Set<string>();
    for (const path of recents) {
      if (seen.has(path)) continue;
      seen.add(path);
      const file = byPath.get(path);
      if (!file) continue;
      out.push({ ...file, score: 0, positions: [] });
      if (out.length >= limit) break;
    }
    return out;
  }

  const scored: RankedFile[] = [];
  for (const file of files) {
    const hit = scorePath(query, file.relative, file.name);
    if (!hit) continue;
    const recency = recentRank.get(file.path);
    const score =
      hit.score + (recency == null ? 0 : (MAX_RECENTS - recency) * 8);
    scored.push({ ...file, score, positions: hit.positions });
  }

  scored.sort((a, b) => {
    if (b.score !== a.score) return b.score - a.score;
    if (a.relative.length !== b.relative.length) {
      return a.relative.length - b.relative.length;
    }
    return a.relative.localeCompare(b.relative);
  });
  return scored.slice(0, limit);
}

/** Resolve a transcript or markdown file link to an existing project file. */
export async function resolveOpenablePath(
  cwd: string,
  href: string,
): Promise<string | undefined> {
  const direct = resolveWorkspacePath(href, cwd);
  if (!direct) return undefined;

  const files = await loadProjectFiles(cwd);
  if (files.length === 0) return direct;

  const byPath = new Map(
    files.map((file) => [normalizeEditorPath(file.path), file]),
  );
  const normalizedDirect = normalizeEditorPath(direct);
  const exact = byPath.get(normalizedDirect);
  if (exact) return exact.path;

  const relHint = relativePathHint(href, cwd, direct);
  const exactRelative = files.find(
    (file) =>
      file.relative === relHint ||
      normalizeEditorPath(file.relative) === relHint,
  );
  if (exactRelative) return exactRelative.path;

  const suffixMatches = files.filter(
    (file) =>
      file.relative === relHint ||
      file.relative.endsWith(`/${relHint}`) ||
      relHint.endsWith(file.relative),
  );
  if (suffixMatches.length === 1) return suffixMatches[0].path;

  const baseName = relHint.split("/").filter(Boolean).pop() ?? relHint;
  const byName = files.filter((file) => file.name === baseName);
  if (byName.length === 0) return direct;
  if (byName.length === 1) return byName[0].path;

  return pickOpenableFile(byName, cwd, relHint).path;
}

function relativePathHint(href: string, cwd: string, direct: string): string {
  let value = href.trim().replace(/\\/g, "/");
  value = value.replace(
    /(?::\d+(?::\d+)?|#L\d+(?:-L\d+)?)$/,
    "",
  );
  if (value.startsWith("file://")) {
    try {
      value = decodeURIComponent(value.slice("file://".length));
    } catch {
      value = value.slice("file://".length);
    }
    value = value.replace(/\\/g, "/");
  }
  value = value.replace(/^\.\//, "").replace(/^\/+/, "");

  const base = cwd.replace(/\\/g, "/").replace(/\/+$/, "");
  const normalizedDirect = normalizeEditorPath(direct);
  if (base && base !== "~" && normalizedDirect.startsWith(`${base}/`)) {
    return normalizedDirect.slice(base.length + 1);
  }
  return value;
}

function pickOpenableFile(
  candidates: ProjectFile[],
  cwd: string,
  relHint: string,
): ProjectFile {
  const recents = recentOpenedFiles(cwd);
  for (const recent of recents) {
    const normalizedRecent = normalizeEditorPath(recent);
    const hit = candidates.find(
      (file) => normalizeEditorPath(file.path) === normalizedRecent,
    );
    if (hit) return hit;
  }

  const suffixMatches = candidates.filter(
    (file) =>
      file.relative === relHint || file.relative.endsWith(`/${relHint}`),
  );
  if (suffixMatches.length > 0) {
    return suffixMatches.sort(
      (a, b) => a.relative.length - b.relative.length,
    )[0];
  }

  return candidates.sort((a, b) => a.relative.length - b.relative.length)[0];
}

subscribeDirsChanged(scheduleIndexRefresh);

if (typeof document !== "undefined") {
  window.addEventListener("focus", () => {
    if (!document.hidden) scheduleIndexRefresh();
  });
  document.addEventListener("visibilitychange", () => {
    if (!document.hidden) scheduleIndexRefresh();
  });
}
