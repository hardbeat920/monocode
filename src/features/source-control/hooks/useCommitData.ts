import { useEffect, useState } from "react";
import type { CommitCache } from "../model/commitCache";

/**
 * Lazily loads a per-commit value, reading the cache synchronously so a value
 * another card already paid for paints without a loading frame.
 *
 * `undefined` means loading, `null` means Git could not report it, otherwise
 * the value. Callers mount this only for a visible card, so the fetch stays
 * hover-driven.
 */
export function useCommitData<T>(
  cwd: string,
  sha: string,
  cache: CommitCache<T>,
): T | null | undefined {
  const [value, setValue] = useState<T | null | undefined>(() =>
    cache.peek(cwd, sha),
  );

  useEffect(() => {
    if (!cwd || cwd === "~" || !sha) {
      setValue(undefined);
      return;
    }
    const cached = cache.peek(cwd, sha);
    if (cached) {
      setValue(cached);
      return;
    }
    let cancelled = false;
    setValue(undefined);
    void cache.load(cwd, sha).then((next) => {
      if (!cancelled) setValue(next);
    });
    return () => {
      cancelled = true;
    };
  }, [cwd, sha, cache]);

  return value;
}
