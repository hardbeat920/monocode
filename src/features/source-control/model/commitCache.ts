/**
 * Roughly a full page of history per repository, several projects over.
 */
export const MAX_CACHED_COMMITS = 256;

export type CommitCache<T> = {
  /** A cached value, or undefined when it has not been loaded yet. */
  peek: (cwd: string, sha: string) => T | undefined;
  /**
   * Load a commit's value, deduping concurrent requests for the same commit.
   * Resolves to null when Git cannot report the commit.
   */
  load: (cwd: string, sha: string) => Promise<T | null>;
  /** Drop everything, so a caller is not served a value from an earlier test. */
  clear: () => void;
};

/**
 * A per-commit cache shared by everything the hover card needs from Git.
 *
 * Commits are immutable, so a successful lookup is cached for the session.
 * Failures are not cached: the next hover retries rather than pinning a
 * transient Git error. The map is capped rather than cleared per project, which
 * bounds it however many repositories a session visits. Map iteration is
 * insertion-ordered, so the oldest entries are the ones dropped.
 */
export function createCommitCache<T>(
  fetchValue: (cwd: string, sha: string) => Promise<T>,
  max = MAX_CACHED_COMMITS,
): CommitCache<T> {
  const cache = new Map<string, T>();
  const inFlight = new Map<string, Promise<T | null>>();

  // NUL cannot appear in a path, so it cannot make two pairs collide.
  const keyOf = (cwd: string, sha: string) => `${cwd}\u0000${sha}`;

  const remember = (key: string, value: T) => {
    cache.set(key, value);
    for (const stale of cache.keys()) {
      if (cache.size <= max) break;
      cache.delete(stale);
    }
  };

  return {
    peek: (cwd, sha) => cache.get(keyOf(cwd, sha)),

    load: (cwd, sha) => {
      const key = keyOf(cwd, sha);
      const cached = cache.get(key);
      if (cached) return Promise.resolve(cached);

      const pending = inFlight.get(key);
      if (pending) return pending;

      const request = fetchValue(cwd, sha)
        .then((value) => {
          remember(key, value);
          return value;
        })
        .catch(() => null)
        .finally(() => {
          inFlight.delete(key);
        });

      inFlight.set(key, request);
      return request;
    },

    clear: () => {
      cache.clear();
      inFlight.clear();
    },
  };
}
