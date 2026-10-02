import { useSyncExternalStore } from "react";
import {
  errorRateLimits,
  fetchingRateLimits,
  idleRateLimits,
  type ProviderRateLimits,
  type RateLimitProvider,
  RATE_LIMIT_MIN_REFETCH_MS,
  RATE_LIMIT_POLL_MS,
} from "./rateLimits";
import {
  fetchClaudeRateLimits,
  fetchCodexRateLimits,
  fetchOpencodeGoRateLimits,
} from "./rateLimitsFetch";

import { fetchPiUsage } from "./piUsage";

export type RateLimitSource =
  RateLimitProvider | "pi:anthropic" | "pi:openai-codex";

const attemptedAt = new Map<string, number>();
const snapshots = new Map<string, ProviderRateLimits>();
const pending = new Map<string, Promise<ProviderRateLimits>>();
const queuedRefreshes = new Map<string, Promise<ProviderRateLimits>>();
const listeners = new Set<() => void>();
let allSnapshots: Record<string, ProviderRateLimits> = {};

function keyFor(provider: RateLimitSource, accountId: string): string {
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
  provider: RateLimitSource,
  accountId = "default",
): ProviderRateLimits {
  return snapshots.get(keyFor(provider, accountId)) ?? idle[provider];
}

const idle: Record<RateLimitSource, ProviderRateLimits> = {
  claude: idleRateLimits("claude"),
  codex: idleRateLimits("codex"),
  opencode: idleRateLimits("opencode"),
  "pi:anthropic": idleRateLimits("claude"),
  "pi:openai-codex": idleRateLimits("codex"),
};

export function useCachedRateLimits(
  provider: RateLimitSource,
  accountId = "default",
): ProviderRateLimits {
  return useSyncExternalStore(
    subscribeRateLimits,
    () => getCachedRateLimits(provider, accountId),
    () => getCachedRateLimits(provider, accountId),
  );
}

export function setCachedRateLimits(
  provider: RateLimitSource,
  accountId: string,
  value: ProviderRateLimits,
): void {
  publish(keyFor(provider, accountId), value);
}

/** Fetch an account once per window lifetime, or again on explicit refresh. */
export function loadRateLimits(
  provider: RateLimitSource,
  accountId = "default",
  force: boolean | "throttled" = false,
): Promise<ProviderRateLimits> {
  const key = keyFor(provider, accountId);
  const running = pending.get(key);
  if (running) {
    if (!force || force === "throttled") return running;
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

  // Operator refreshes join UI work and respect the shared attempt cooldown.
  // Failed/unavailable probes back off for a poll interval. Explicit UI actions
  // (e.g. signing in or redeeming a reset) may still force an immediate read.
  const lastAttempt = attemptedAt.get(key) ?? cached?.updatedAt;
  const cooldown =
    cached?.status === "ok" ? RATE_LIMIT_MIN_REFETCH_MS : RATE_LIMIT_POLL_MS;
  if (
    force === "throttled" &&
    cached &&
    lastAttempt != null &&
    Date.now() - lastAttempt < cooldown
  ) {
    return Promise.resolve(cached);
  }
  attemptedAt.set(key, Date.now());
  const billingProvider = idle[provider].provider;
  publish(key, {
    ...fetchingRateLimits(billingProvider, cached),
    fetchedAt:
      cached?.fetchedAt ?? (cached?.status === "ok" ? cached.updatedAt : null),
  });
  const run = (async () => {
    try {
      let result =
        provider === "claude"
          ? await fetchClaudeRateLimits(accountId)
          : provider === "codex"
            ? await fetchCodexRateLimits(accountId)
            : provider === "opencode"
              ? await fetchOpencodeGoRateLimits()
              : await fetchPiUsage(
                  provider === "pi:anthropic" ? "anthropic" : "openai-codex",
                );
      if (result.status === "ok") {
        result = { ...result, fetchedAt: result.updatedAt };
      } else if (cached) {
        result = {
          ...cached,
          status: result.status,
          error: result.error,
          updatedAt: result.updatedAt,
          fetchedAt:
            cached.fetchedAt ??
            (cached.status === "ok" ? cached.updatedAt : null),
        };
      }
      publish(key, result);
      return result;
    } catch (error) {
      const result = errorRateLimits(
        billingProvider,
        error instanceof Error ? error.message : String(error),
        getCachedRateLimits(provider, accountId),
      );
      result.fetchedAt =
        cached?.fetchedAt ??
        (cached?.status === "ok" ? cached.updatedAt : null);
      publish(key, result);
      return result;
    } finally {
      pending.delete(key);
    }
  })();
  pending.set(key, run);
  return run;
}

/** Also used when an account is removed and by tests that need a clean cache. */
export function clearCachedRateLimits(
  provider?: RateLimitSource,
  accountId?: string,
): void {
  if (provider && accountId) {
    const key = keyFor(provider, accountId);
    snapshots.delete(key);
    attemptedAt.delete(key);
    const { [key]: _removed, ...rest } = allSnapshots;
    allSnapshots = rest;
  } else {
    snapshots.clear();
    attemptedAt.clear();
    allSnapshots = {};
  }
  for (const listener of listeners) listener();
}
