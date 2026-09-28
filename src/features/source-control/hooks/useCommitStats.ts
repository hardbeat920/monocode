import { useEffect, useState } from "react";
import {
  loadCommitStats,
  peekCommitStats,
  type CommitStats,
} from "../model/commitStats";

/**
 * Lazily loads a commit's changed-file summary. `undefined` means loading,
 * `null` means Git could not report it, otherwise the summary. Callers mount
 * this only for a visible card, so the fetch stays hover-driven.
 */
export function useCommitStats(
  cwd: string,
  sha: string,
): CommitStats | null | undefined {
  const [stats, setStats] = useState<CommitStats | null | undefined>(() =>
    peekCommitStats(cwd, sha),
  );

  useEffect(() => {
    if (!cwd || cwd === "~" || !sha) {
      setStats(undefined);
      return;
    }
    const cached = peekCommitStats(cwd, sha);
    if (cached) {
      setStats(cached);
      return;
    }
    let cancelled = false;
    setStats(undefined);
    void loadCommitStats(cwd, sha).then((value) => {
      if (!cancelled) setStats(value);
    });
    return () => {
      cancelled = true;
    };
  }, [cwd, sha]);

  return stats;
}
