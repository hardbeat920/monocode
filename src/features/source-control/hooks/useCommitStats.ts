import { useCommitData } from "./useCommitData";
import { commitStatsCache, type CommitStats } from "../model/commitStats";

/** Lazily loads a commit's changed-file summary for the hover card. */
export function useCommitStats(
  cwd: string,
  sha: string,
): CommitStats | null | undefined {
  return useCommitData(cwd, sha, commitStatsCache);
}
