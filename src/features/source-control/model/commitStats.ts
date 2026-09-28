import {
  gitCommitFiles,
  type GitChangedFile,
} from "../../../platform/tauri/fs";
import { createCommitCache, MAX_CACHED_COMMITS } from "./commitCache";

export type CommitStats = {
  filesChanged: number;
  additions: number;
  deletions: number;
};

/** Alias kept so the cap reads as being about stats at the call site. */
export const MAX_CACHED_COMMIT_STATS = MAX_CACHED_COMMITS;

/**
 * Module-level so every card for the same commit shares one Git call, and so
 * the identity is stable for the hook's dependency list.
 */
export const commitStatsCache = createCommitCache<CommitStats>(
  async (cwd, sha) => summarizeCommitFiles(await gitCommitFiles(cwd, sha)),
);

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
  return commitStatsCache.peek(cwd, sha);
}

/** Load a commit's summary. Resolves to null when Git cannot report it. */
export function loadCommitStats(
  cwd: string,
  sha: string,
): Promise<CommitStats | null> {
  return commitStatsCache.load(cwd, sha);
}

/** Drop the cache, so one test cannot be served stats loaded by another. */
export function clearCommitStatsCache(): void {
  commitStatsCache.clear();
}
