import { useSyncExternalStore } from "react";
import {
  errorRateLimits,
  fetchingRateLimits,
  idleRateLimits,
  parseCodexRateLimits,
  type RateLimitWindow,
  type ProviderRateLimits,
  type RateLimitProvider,
} from "./rateLimits";
import {
  fetchClaudeRateLimits,
  fetchCodexRateLimits,
  fetchDevinRateLimits,
  fetchOpencodeGoRateLimits,
} from "./rateLimitsFetch";
import { providerAccountExists } from "./providerAccounts";

const snapshots = new Map<string, ProviderRateLimits>();
const pending = new Map<string, Promise<ProviderRateLimits>>();
const queuedRefreshes = new Map<string, Promise<ProviderRateLimits>>();
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

/** App-server pushes account quota changes during a turn; no usage probe is needed. */
export function updateCodexRateLimits(
  update: Record<string, unknown>,
  accountId = "default",
): void {
  // Model-specific buckets must not replace the account's main Codex meter.
  if (update.limitId && update.limitId !== "codex") return;
  if (!providerAccountExists("codex", accountId)) return;
  const parsed = parseCodexRateLimits(update);
  if (!parsed.session && !parsed.weekly && !parsed.monthly) return;
  const cached = getCachedRateLimits("codex", accountId);
  publish(keyFor("codex", accountId), {
    ...parsed,
    session: mergeCodexWindow(parsed.session, cached.session),
    weekly: mergeCodexWindow(parsed.weekly, cached.weekly),
    monthly: mergeCodexWindow(parsed.monthly, cached.monthly),
    resetCredits: cached.resetCredits,
  });
}

/** Sparse quota pushes may update usage without supplying a new reset time. */
function mergeCodexWindow(
  window: RateLimitWindow | null,
  cached: RateLimitWindow | null,
): RateLimitWindow | null {
  return window
    ? { ...window, resetsAt: window.resetsAt ?? cached?.resetsAt ?? null }
    : cached;
}

/** Reuse the cached account snapshot unless the caller requests a fresh read. */
export function loadRateLimits(
  provider: RateLimitProvider,
  accountId = "default",
  force = false,
): Promise<ProviderRateLimits> {
  const key = keyFor(provider, accountId);
  if (force) {
    cancelRateLimitsAutoRefresh(provider, accountId);
  }
  const running = pending.get(key);
  if (running) {
    if (!force) return running;
    const queued = queuedRefreshes.get(key);
    if (queued) return queued;
    const next = running.then(() => {
      // Once this refresh starts, a newer completion must be able to queue another.
      if (queuedRefreshes.get(key) === next) queuedRefreshes.delete(key);
      return loadRateLimits(provider, accountId, true);
    });
    queuedRefreshes.set(key, next);
    void next.finally(() => {
      if (queuedRefreshes.get(key) === next) queuedRefreshes.delete(key);
    });
    return next;
  }
  const cached = snapshots.get(key);
  if (cached && !force) return Promise.resolve(cached);

  const fetching = fetchingRateLimits(provider, cached);
  publish(key, fetching);
  const run = (async () => {
    try {
      const result =
        provider === "claude"
          ? await fetchClaudeRateLimits(accountId)
          : provider === "codex"
            ? await fetchCodexRateLimits(accountId)
            : provider === "devin"
              ? await fetchDevinRateLimits()
              : await fetchOpencodeGoRateLimits();
      // A pushed snapshot or account removal supersedes this older probe.
      if (snapshots.get(key) !== fetching) return snapshots.get(key) ?? result;
      publish(key, result);
      return result;
    } catch (error) {
      if (snapshots.get(key) !== fetching)
        return snapshots.get(key) ?? idle[provider];
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

const autoRefreshTimers = new Map<string, ReturnType<typeof setTimeout>>();

export const AUTO_REFRESH_DEBOUNCE_MS = 1500;

/** Debounces rate-limit refresh per provider:account to combine closely spaced completions. */
export function scheduleRateLimitsAutoRefresh(
  provider: RateLimitProvider,
  accountId = "default",
  debounceMs = AUTO_REFRESH_DEBOUNCE_MS,
): () => void {
  const key = keyFor(provider, accountId);
  const existing = autoRefreshTimers.get(key);
  if (existing) clearTimeout(existing);

  const timer = setTimeout(() => {
    autoRefreshTimers.delete(key);
    void loadRateLimits(provider, accountId, true);
  }, debounceMs);

  autoRefreshTimers.set(key, timer);
  return () => {
    if (autoRefreshTimers.get(key) === timer) {
      clearTimeout(timer);
      autoRefreshTimers.delete(key);
    }
  };
}

export function cancelRateLimitsAutoRefresh(
  provider?: RateLimitProvider,
  accountId?: string,
): void {
  if (provider && accountId) {
    const key = keyFor(provider, accountId);
    const existing = autoRefreshTimers.get(key);
    if (existing) {
      clearTimeout(existing);
      autoRefreshTimers.delete(key);
    }
  } else {
    for (const timer of autoRefreshTimers.values()) clearTimeout(timer);
    autoRefreshTimers.clear();
  }
}

/** Also used when an account is removed and by tests that need a clean cache. */
export function clearCachedRateLimits(
  provider?: RateLimitProvider,
  accountId?: string,
): void {
  cancelRateLimitsAutoRefresh(provider, accountId);
  if (provider && accountId) {
    const key = keyFor(provider, accountId);
    snapshots.delete(key);
    const { [key]: _removed, ...rest } = allSnapshots;
    allSnapshots = rest;
  } else {
    snapshots.clear();
    allSnapshots = {};
  }
  for (const listener of listeners) listener();
}
