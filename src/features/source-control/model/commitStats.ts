import {
  gitCommitFiles,
  type GitChangedFile,
} from "../../../platform/tauri/fs";

export type CommitStats = {
  filesChanged: number;
  additions: number;
  deletions: number;
};

/**
 * Commits are immutable, so a successful lookup is cached for the session.
 * Failures are not cached: the next hover retries rather than pinning a
 * transient Git error.
 *
 * The map is capped rather than cleared per project, which bounds it no matter
 * how many repositories a session visits without needing call sites to tell it
 * when the project changed. Map iteration is insertion-ordered, so the oldest
 * entries are the ones dropped.
 */
const cache = new Map<string, CommitStats>();
const inFlight = new Map<string, Promise<CommitStats | null>>();

/** Roughly a full page of history per repository, several projects over. */
export const MAX_CACHED_COMMIT_STATS = 256;

function cacheKey(cwd: string, sha: string): string {
  return `${cwd}\u0000${sha}`;
}

function remember(key: string, value: CommitStats): void {
  cache.set(key, value);
  for (const stale of cache.keys()) {
    if (cache.size <= MAX_CACHED_COMMIT_STATS) break;
    cache.delete(stale);
  }
}

/** Changed-file counts for the commit summary line. */
export function summarizeCommitFiles(files: GitChangedFile[]): CommitStats {
  let additions = 0;
  let deletions = 0;
  for (const file of files) {
    additions += Math.max(0, file.additions);
    deletions += Math.max(0, file.deletions);
  }
  return { filesChanged: files.length, additions, deletions };
}

/** A cached summary, or undefined when it has not been loaded yet. */
export function peekCommitStats(
  cwd: string,
  sha: string,
): CommitStats | undefined {
  return cache.get(cacheKey(cwd, sha));
}

/**
 * Load a commit's changed-file summary, deduping concurrent requests for the
 * same commit. Resolves to null when Git cannot report the commit.
 */
export function loadCommitStats(
  cwd: string,
  sha: string,
): Promise<CommitStats | null> {
  const key = cacheKey(cwd, sha);
  const cached = cache.get(key);
  if (cached) return Promise.resolve(cached);

  const pending = inFlight.get(key);
  if (pending) return pending;

  const request = gitCommitFiles(cwd, sha)
    .then((files) => {
      const stats = summarizeCommitFiles(files);
      remember(key, stats);
      return stats;
    })
    .catch(() => null)
    .finally(() => {
      inFlight.delete(key);
    });

  inFlight.set(key, request);
  return request;
}
