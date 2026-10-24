import { useSyncExternalStore } from "react";
import {
  errorRateLimits,
  fetchingRateLimits,
  idleRateLimits,
  mergeCodexRateLimitsUpdate,
  RATE_LIMIT_MIN_REFETCH_MS,
  type ProviderRateLimits,
  type RateLimitProvider,
} from "./rateLimits";
import {
  fetchClaudeRateLimits,
  fetchCodexRateLimits,
  fetchDevinRateLimits,
  fetchOpencodeGoRateLimits,
} from "./rateLimitsFetch";

const snapshots = new Map<string, ProviderRateLimits>();
const pending = new Map<string, Promise<ProviderRateLimits>>();
const queuedRefreshes = new Map<string, Promise<ProviderRateLimits>>();
const lastAttemptAt = new Map<string, number>();
const listeners = new Set<() => void>();
let allSnapshots: Record<string, ProviderRateLimits> = {};

function keyFor(provider: RateLimitProvider, accountId: string): string {
  return `${provider}:${accountId}`;
}

function publish(key: string, value: ProviderRateLimits): void {
  snapshots.set(key, value);
  allSnapshots = { ...allSnapshots, [key]: value };
  for (const listener of listeners) listener();
}

export function subscribeRateLimits(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function getAllRateLimits(): Record<string, ProviderRateLimits> {
  return allSnapshots;
}

export function getCachedRateLimits(
  provider: RateLimitProvider,
  accountId = "default",
): ProviderRateLimits {
  return snapshots.get(keyFor(provider, accountId)) ?? idle[provider];
}

const idle: Record<RateLimitProvider, ProviderRateLimits> = {
  claude: idleRateLimits("claude"),
  codex: idleRateLimits("codex"),
  opencode: idleRateLimits("opencode"),
  devin: idleRateLimits("devin"),
};

export function useCachedRateLimits(
  provider: RateLimitProvider,
  accountId = "default",
): ProviderRateLimits {
  return useSyncExternalStore(
    subscribeRateLimits,
    () => getCachedRateLimits(provider, accountId),
    () => getCachedRateLimits(provider, accountId),
  );
}

export function setCachedRateLimits(
  provider: RateLimitProvider,
  accountId: string,
  value: ProviderRateLimits,
): void {
  publish(keyFor(provider, accountId), value);
}

/** Fetch an account once per window lifetime, or again on explicit refresh. */
export function loadRateLimits(
  provider: RateLimitProvider,
  accountId = "default",
  force = false,
): Promise<ProviderRateLimits> {
  const key = keyFor(provider, accountId);
  const running = pending.get(key);
  if (running) {
    if (!force) return running;
    const queued = queuedRefreshes.get(key);
    if (queued) return queued;
    const next = running.then(() => loadRateLimits(provider, accountId, true));
    queuedRefreshes.set(key, next);
    void next.finally(() => {
      if (queuedRefreshes.get(key) === next) queuedRefreshes.delete(key);
    });
    return next;
  }
  const cached = snapshots.get(key);
  if (cached && !force) return Promise.resolve(cached);

  publish(key, fetchingRateLimits(provider, cached));
  lastAttemptAt.set(key, Date.now());
  const run = (async () => {
    try {
      const fetched =
        provider === "claude"
          ? await fetchClaudeRateLimits(accountId)
          : provider === "codex"
            ? await fetchCodexRateLimits(accountId)
            : provider === "devin"
              ? await fetchDevinRateLimits()
              : await fetchOpencodeGoRateLimits();
      // A failed refresh keeps the last good windows beside its error.
      const result =
        fetched.status === "error"
          ? errorRateLimits(
              provider,
              fetched.error ?? "Usage unavailable",
              getCachedRateLimits(provider, accountId),
            )
          : fetched;
      publish(key, result);
      return result;
    } catch (error) {
      const result = errorRateLimits(
        provider,
        error instanceof Error ? error.message : String(error),
        getCachedRateLimits(provider, accountId),
      );
      publish(key, result);
      return result;
    } finally {
      pending.delete(key);
    }
  })();
  pending.set(key, run);
  return run;
}

/** How soon a reading that did not succeed may be retried after a turn. */
const FAILED_READ_RETRY_MS = 60_000;

/**
 * Refetch after an agent turn when the footer's reading is old. Accounts the
 * footer never loaded are left alone, and refetches are spaced out so busy
 * sessions do not hammer the usage endpoint.
 */
export function refreshRateLimitsIfStale(
  provider: RateLimitProvider,
  accountId = "default",
): void {
  const key = keyFor(provider, accountId);
  const cached = snapshots.get(key);
  if (!cached || cached.status === "idle" || pending.has(key)) return;
  const interval =
    cached.status === "ok" ? RATE_LIMIT_MIN_REFETCH_MS : FAILED_READ_RETRY_MS;
  if (Date.now() - (lastAttemptAt.get(key) ?? 0) < interval) return;
  void loadRateLimits(provider, accountId, true);
}

/** Publish a running Codex session's live rate-limit windows. */
export function applyLiveCodexRateLimits(
  accountId: string,
  update: Record<string, unknown>,
): void {
  const merged = mergeCodexRateLimitsUpdate(
    getCachedRateLimits("codex", accountId),
    update,
  );
  if (merged) publish(keyFor("codex", accountId), merged);
}

/** Also used when an account is removed and by tests that need a clean cache. */
export function clearCachedRateLimits(
  provider?: RateLimitProvider,
  accountId?: string,
): void {
  if (provider && accountId) {
    const key = keyFor(provider, accountId);
    snapshots.delete(key);
    lastAttemptAt.delete(key);
    const { [key]: _removed, ...rest } = allSnapshots;
    allSnapshots = rest;
  } else {
    snapshots.clear();
    lastAttemptAt.clear();
    allSnapshots = {};
  }
  for (const listener of listeners) listener();
}
