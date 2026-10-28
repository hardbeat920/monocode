import { useCallback, useEffect, useState, useSyncExternalStore } from "react";
import {
  clampUsedPercent,
  exhaustedWindowResetAt,
  formatResetDuration,
  type ProviderRateLimits,
} from "./rateLimits";
import {
  getAllRateLimits,
  loadRateLimits,
  subscribeRateLimits,
} from "./rateLimitsCache";
import {
  PROVIDER_ACCOUNT_PROVIDERS,
  providerAccounts,
  type ProviderAccount,
  type ProviderAccountProvider,
} from "./providerAccounts";
import { identityKey } from "./providerAccountIdentity";

const CLOCK_MS = 30_000;
/** At or below this much headroom an account reads as "Running low". */
export const LOW_HEADROOM_PERCENT = 20;

/** Same `provider:id` key the identity cache uses. */
export const accountUsageKey = identityKey;

export type AccountStatusTone =
  "ready" | "low" | "exhausted" | "checking" | "unknown";

export type AccountStatus = {
  tone: AccountStatusTone;
  label: string;
  /** Extra context, e.g. "back in 31m" for an exhausted account. */
  detail: string | null;
};

/**
 * Remaining percent of the tightest window, or null without usage data.
 * A window whose reset time has passed counts as fully available.
 */
export function accountHeadroom(
  limits: ProviderRateLimits | undefined,
  now: number,
): number | null {
  const windows = [limits?.session, limits?.weekly, limits?.monthly].filter(
    (window) => window != null,
  );
  if (windows.length === 0) return null;
  return Math.min(
    ...windows.map((window) =>
      window.resetsAt != null && window.resetsAt <= now
        ? 100
        : 100 - clampUsedPercent(window.usedPercent),
    ),
  );
}

/** Ready / Running low / Exhausted for an account, shared by every surface. */
export function accountStatus(
  limits: ProviderRateLimits | undefined,
  now: number,
): AccountStatus {
  const headroom = accountHeadroom(limits, now);
  if (!limits || headroom == null) {
    if (!limits || limits.status === "idle" || limits.status === "fetching") {
      return { tone: "checking", label: "Checking…", detail: null };
    }
    return {
      tone: "unknown",
      label:
        limits.status === "unavailable"
          ? limits.error || "Not signed in"
          : limits.error || "Usage unavailable",
      detail: null,
    };
  }
  if (headroom <= 0) {
    return {
      tone: "exhausted",
      label: "Exhausted",
      detail: backIn(limits, now),
    };
  }
  if (headroom <= LOW_HEADROOM_PERCENT) {
    return {
      tone: "low",
      label: "Running low",
      detail: `${Math.round(headroom)}% left`,
    };
  }
  return { tone: "ready", label: "Ready", detail: null };
}

/** "back in 31m" for the used-up window that stays blocked longest. */
function backIn(limits: ProviderRateLimits, now: number): string | null {
  const resetAt = exhaustedWindowResetAt(limits);
  if (resetAt == null || resetAt <= now) return null;
  return `back in ${formatResetDuration(resetAt - now)}`;
}

/** The account with the most headroom, if it is comfortably above "low". */
export function bestAlternativeAccount(
  accounts: ProviderAccount[],
  usageFor: (account: ProviderAccount) => ProviderRateLimits | undefined,
  now: number,
): ProviderAccount | null {
  let best: { account: ProviderAccount; headroom: number } | null = null;
  for (const account of accounts) {
    const headroom = accountHeadroom(usageFor(account), now);
    if (headroom == null || headroom <= LOW_HEADROOM_PERCENT) continue;
    if (!best || headroom > best.headroom) best = { account, headroom };
  }
  return best?.account ?? null;
}

function accountsFor(provider?: ProviderAccountProvider): ProviderAccount[] {
  return provider
    ? providerAccounts(provider)
    : PROVIDER_ACCOUNT_PROVIDERS.flatMap((entry) => providerAccounts(entry));
}

export type AccountUsage = {
  usage: Record<string, ProviderRateLimits>;
  now: number;
  refreshing: boolean;
  refresh: () => void;
};

/**
 * Usage windows for every account of `provider` (or of every provider), so
 * Settings and the footer account picker can show which account has
 * headroom. `accountsVersion` should change when accounts are added or
 * removed. While `enabled`, accounts without a window snapshot load once;
 * `refresh` explicitly reloads every account.
 */
export function useProviderAccountUsage(
  accountsVersion: unknown,
  {
    provider,
    enabled = true,
  }: { provider?: ProviderAccountProvider; enabled?: boolean } = {},
): AccountUsage {
  const usage = useSyncExternalStore(
    subscribeRateLimits,
    getAllRateLimits,
    getAllRateLimits,
  );
  const [inflight, setInflight] = useState(0);
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), CLOCK_MS);
    return () => window.clearInterval(timer);
  }, []);

  const load = useCallback(
    async (targets: ProviderAccount[], force = false) => {
      if (targets.length === 0) return;
      setInflight((count) => count + 1);
      try {
        await Promise.allSettled(
          targets.map((account) =>
            loadRateLimits(account.provider, account.id, force),
          ),
        );
      } finally {
        setInflight((count) => count - 1);
        setNow(Date.now());
      }
    },
    [],
  );

  // Renames also bump the version; the shared cache prevents repeat probes.
  useEffect(() => {
    if (!enabled) return;
    const cached = getAllRateLimits();
    void load(
      accountsFor(provider).filter(
        (account) => !cached[accountUsageKey(account)],
      ),
    );
  }, [accountsVersion, enabled, load, provider]);

  const refresh = useCallback(
    () => void load(accountsFor(provider), true),
    [load, provider],
  );

  return { usage, now, refreshing: inflight > 0, refresh };
}
