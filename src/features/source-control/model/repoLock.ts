/**
 * One Git mutation at a time per repository, held outside React so it
 * survives panels remounting, switching projects or closing mid-operation.
 * Each acquisition gets its own release, so a stale handler finishing late
 * never frees a lock a newer operation now holds.
 */
export type RepoLock = { kind: string; owner: object };

const locks = new Map<string, RepoLock>();
const listeners = new Set<() => void>();

const key = (repo: string) => repo.replace(/\/+$/, "") || "/";

export function repoLock(repo: string): RepoLock | null {
  return locks.get(key(repo)) ?? null;
}

/**
 * Take the repository's lock for one operation. Returns its release, or null
 * when anything already holds it; callers must not start work on null.
 * `owner` lets a component recognize its own operation in `repoLock()`.
 */
export function acquireRepoLock(
  repo: string,
  kind: string,
  owner: object = {},
): (() => void) | null {
  const id = key(repo);
  if (locks.has(id)) return null;
  const lock: RepoLock = { kind, owner };
  locks.set(id, lock);
  emit();
  return () => {
    if (locks.get(id) !== lock) return;
    locks.delete(id);
    emit();
  };
}

export function subscribeRepoLocks(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

function emit() {
  for (const listener of listeners) listener();
}
