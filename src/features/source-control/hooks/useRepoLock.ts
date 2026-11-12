import { useCallback, useSyncExternalStore } from "react";
import {
  acquireRepoLock,
  repoLock,
  subscribeRepoLocks,
  type RepoLock,
} from "../model/repoLock";

/** Takes the repository's lock for one operation; null means don't start. */
export type AcquireRepoLock = (
  kind: string,
  owner?: object,
) => (() => void) | null;

/**
 * A component's view of its repository's Git lock. `busy` is whatever any
 * mount is doing; `acquire` must succeed before starting a mutation.
 */
export function useRepoLock(repo: string): {
  lock: RepoLock | null;
  busy: string | null;
  acquire: AcquireRepoLock;
} {
  const lock = useSyncExternalStore(subscribeRepoLocks, () => repoLock(repo));
  const acquire = useCallback<AcquireRepoLock>(
    (kind, owner) => acquireRepoLock(repo, kind, owner),
    [repo],
  );
  return { lock, busy: lock?.kind ?? null, acquire };
}
