import {
  fetchClaudeRateLimits,
  fetchCodexRateLimits,
  fetchCursorRateLimits,
  fetchGrokRateLimits,
} from "./rateLimitsFetch";
import {
  fetchingRateLimits,
  idleRateLimits,
  RATE_LIMIT_POLL_MS,
  RATE_LIMIT_PROVIDERS,
  shouldFetchProvider,
  type ProviderRateLimits,
  type RateLimitProvider,
} from "./rateLimits";

export const RATE_LIMITS_CACHE_KEY = "monocode.rateLimits.cache";
export const RATE_LIMITS_LOCK_KEY = "monocode.rateLimits.lock";
const LOCK_TTL_MS = 30_000;

export type RateLimitSnapshot = {
  claude: ProviderRateLimits;
  codex: ProviderRateLimits;
  cursor: ProviderRateLimits;
  grok: ProviderRateLimits;
  refreshing: boolean;
};

export type RateLimitFetcherMap = {
  [K in RateLimitProvider]: () => Promise<ProviderRateLimits>;
};

const defaultFetchers: RateLimitFetcherMap = {
  claude: fetchClaudeRateLimits,
  codex: fetchCodexRateLimits,
  cursor: fetchCursorRateLimits,
  grok: fetchGrokRateLimits,
};

let fetchers = defaultFetchers;
let snapshot = idleSnapshot();
const listeners = new Set<() => void>();
let wanted: RateLimitProvider[] = [];
let inflight: Promise<void> | null = null;
let started = false;
let pollTimer: ReturnType<typeof setInterval> | undefined;

function idleSnapshot(): RateLimitSnapshot {
  return {
    claude: idleRateLimits("claude"),
    codex: idleRateLimits("codex"),
    cursor: idleRateLimits("cursor"),
    grok: idleRateLimits("grok"),
    refreshing: false,
  };
}

function emit() {
  for (const listener of listeners) listener();
}

function replace(next: RateLimitSnapshot) {
  snapshot = next;
  emit();
}

function isVisible(): boolean {
  return typeof document === "undefined" || document.visibilityState !== "hidden";
}

function readCache(): Partial<Record<RateLimitProvider, ProviderRateLimits>> | null {
  try {
    const raw = localStorage.getItem(RATE_LIMITS_CACHE_KEY);
    if (!raw) return null;
    const parsed: unknown = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object") return null;
    const rec = parsed as Record<string, unknown>;
    const out: Partial<Record<RateLimitProvider, ProviderRateLimits>> = {};
    for (const provider of RATE_LIMIT_PROVIDERS) {
      const limits = asLimits(provider, rec[provider]);
      if (limits) out[provider] = limits;
    }
    return out;
  } catch {
    return null;
  }
}

function asLimits(
  provider: RateLimitProvider,
  value: unknown,
): ProviderRateLimits | null {
  if (!value || typeof value !== "object") return null;
  const rec = value as Record<string, unknown>;
  if (rec.provider !== provider || typeof rec.updatedAt !== "number") {
    return null;
  }
  const status = rec.status;
  if (
    status !== "idle" &&
    status !== "fetching" &&
    status !== "ok" &&
    status !== "error" &&
    status !== "unavailable"
  ) {
    return null;
  }
  return {
    provider,
    session: windowFromUnknown(rec.session),
    weekly: windowFromUnknown(rec.weekly),
    updatedAt: rec.updatedAt,
    error: typeof rec.error === "string" ? rec.error : null,
    status: status === "fetching" ? "ok" : status,
  };
}

function windowFromUnknown(
  value: unknown,
): ProviderRateLimits["session"] {
  return value && typeof value === "object"
    ? (value as ProviderRateLimits["session"])
    : null;
}

function writeCache(current: RateLimitSnapshot) {
  try {
    const payload: Record<string, ProviderRateLimits> = {};
    for (const provider of RATE_LIMIT_PROVIDERS) {
      const limits = current[provider];
      if (limits.status === "idle" || limits.status === "fetching") continue;
      payload[provider] = limits;
    }
    localStorage.setItem(RATE_LIMITS_CACHE_KEY, JSON.stringify(payload));
  } catch {
    // private mode / quota
  }
}

function lockHeld(): boolean {
  try {
    const raw = localStorage.getItem(RATE_LIMITS_LOCK_KEY);
    if (!raw) return false;
    const parsed: unknown = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object") return false;
    const at = (parsed as { at?: unknown }).at;
    return typeof at === "number" && Date.now() - at < LOCK_TTL_MS;
  } catch {
    return false;
  }
}

function acquireLock() {
  try {
    localStorage.setItem(RATE_LIMITS_LOCK_KEY, JSON.stringify({ at: Date.now() }));
  } catch {
    // private mode / quota
  }
}

function releaseLock() {
  try {
    localStorage.removeItem(RATE_LIMITS_LOCK_KEY);
  } catch {
    // private mode / quota
  }
}

function hydrateFromCache() {
  const cached = readCache();
  if (!cached) return;
  replace({
    claude: cached.claude ?? snapshot.claude,
    codex: cached.codex ?? snapshot.codex,
    cursor: cached.cursor ?? snapshot.cursor,
    grok: cached.grok ?? snapshot.grok,
    refreshing: snapshot.refreshing,
  });
}

function onStorage(event: StorageEvent) {
  if (event.key !== RATE_LIMITS_CACHE_KEY) return;
  hydrateFromCache();
}

function onVisible() {
  if (isVisible()) void refreshRateLimits();
}

function ensureStarted() {
  if (started) return;
  started = true;
  hydrateFromCache();
  if (typeof window !== "undefined") {
    window.addEventListener("storage", onStorage);
    document.addEventListener("visibilitychange", onVisible);
  }
  pollTimer = setInterval(() => void refreshRateLimits(), RATE_LIMIT_POLL_MS);
}

export function getRateLimitsSnapshot(): RateLimitSnapshot {
  return snapshot;
}

export function subscribeRateLimits(onStoreChange: () => void) {
  listeners.add(onStoreChange);
  ensureStarted();
  return () => {
    listeners.delete(onStoreChange);
  };
}

export function setRateLimitProviders(providers: RateLimitProvider[]) {
  wanted = [...providers];
  ensureStarted();
  void refreshRateLimits();
}

export function refreshRateLimits(force = false): Promise<void> | undefined {
  ensureStarted();
  if (inflight) {
    if (!force) return inflight;
    if (!snapshot.refreshing) replace({ ...snapshot, refreshing: true });
    return inflight.then(async () => {
      await refreshRateLimits(true);
    });
  }
  const visible = isVisible();
  const pending = RATE_LIMIT_PROVIDERS.filter(
    (provider) =>
      wanted.includes(provider) &&
      shouldFetchProvider(snapshot[provider], { force, visible }),
  );
  if (pending.length === 0) return;
  if (!force && lockHeld()) return;
  acquireLock();

  let next = { ...snapshot, refreshing: force || snapshot.refreshing };
  for (const provider of pending) {
    next = {
      ...next,
      [provider]: fetchingRateLimits(provider, snapshot[provider]),
    };
  }
  replace(next);

  const run = Promise.allSettled(
    pending.map((provider) =>
      fetchers[provider]().then((value) => {
        snapshot = { ...snapshot, [provider]: value };
        emit();
      }),
    ),
  )
    .then(() => undefined)
    .finally(() => {
      inflight = null;
      replace({ ...snapshot, refreshing: false });
      writeCache(snapshot);
      releaseLock();
    });
  inflight = run;
  return run;
}

export function setRateLimitFetchersForTests(next: RateLimitFetcherMap) {
  fetchers = next;
}

export function resetRateLimitsStoreForTests() {
  if (pollTimer) {
    clearInterval(pollTimer);
    pollTimer = undefined;
  }
  if (typeof window !== "undefined") {
    window.removeEventListener("storage", onStorage);
    document.removeEventListener("visibilitychange", onVisible);
  }
  fetchers = defaultFetchers;
  snapshot = idleSnapshot();
  listeners.clear();
  wanted = [];
  inflight = null;
  started = false;
}
