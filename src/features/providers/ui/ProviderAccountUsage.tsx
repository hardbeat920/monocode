import { useCallback, useEffect, useRef, useState } from "react";
import {
  clampUsedPercent,
  exhaustedWindowResetAt,
  fetchingRateLimits,
  formatResetDuration,
  formatUsagePercent,
  formatWindowLabel,
  RATE_LIMIT_MIN_REFETCH_MS,
  type ProviderRateLimits,
  type RateLimitWindow,
} from "../model/rateLimits";
import {
  fetchClaudeRateLimits,
  fetchCodexRateLimits,
} from "../model/rateLimitsFetch";
import {
  PROVIDER_ACCOUNT_PROVIDERS,
  providerAccounts,
  type ProviderAccount,
  type ProviderAccountProvider,
} from "../model/providerAccounts";
import { identityKey } from "../model/providerAccountIdentity";
import { RefreshCw } from "../../../shared/ui/icons";

const CLOCK_MS = 30_000;

export type AccountUsage = {
  usage: Record<string, ProviderRateLimits>;
  now: number;
  refreshing: boolean;
  refresh: () => void;
};

/** Same `provider:id` key the identity cache uses. */
export const accountUsageKey = identityKey;

function accountsFor(provider?: ProviderAccountProvider): ProviderAccount[] {
  return provider
    ? providerAccounts(provider)
    : PROVIDER_ACCOUNT_PROVIDERS.flatMap((entry) => providerAccounts(entry));
}

function fetchAccountUsage(
  account: ProviderAccount,
): Promise<ProviderRateLimits> {
  return account.provider === "claude"
    ? fetchClaudeRateLimits(account.id)
    : fetchCodexRateLimits(account.id);
}

/**
 * Usage windows for every provider account at once, so the Accounts list and
 * the footer account picker can show which profile still has headroom.
 * `accountsVersion` should change whenever accounts are added or removed.
 * While `enabled`, snapshots older than the footer's refetch floor reload.
 */
export function useProviderAccountUsage(
  accountsVersion: unknown,
  {
    provider,
    enabled = true,
  }: { provider?: ProviderAccountProvider; enabled?: boolean } = {},
): AccountUsage {
  const [usage, setUsage] = useState<Record<string, ProviderRateLimits>>({});
  const [refreshing, setRefreshing] = useState(false);
  const [now, setNow] = useState(() => Date.now());
  const requestedAt = useRef(new Map<string, number>());

  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), CLOCK_MS);
    return () => window.clearInterval(timer);
  }, []);

  const load = useCallback(async (targets: ProviderAccount[]) => {
    if (targets.length === 0) return;
    const started = Date.now();
    for (const account of targets) {
      requestedAt.current.set(accountUsageKey(account), started);
    }
    setRefreshing(true);
    setUsage((current) => {
      const next = { ...current };
      for (const account of targets) {
        const key = accountUsageKey(account);
        next[key] = fetchingRateLimits(account.provider, current[key]);
      }
      return next;
    });
    await Promise.allSettled(
      targets.map(async (account) => {
        const value = await fetchAccountUsage(account);
        setUsage((current) => ({
          ...current,
          [accountUsageKey(account)]: value,
        }));
      }),
    );
    setRefreshing(false);
    setNow(Date.now());
  }, []);

  // Renames also bump the version; only fetch accounts missing or stale.
  useEffect(() => {
    if (!enabled) return;
    const cutoff = Date.now() - RATE_LIMIT_MIN_REFETCH_MS;
    void load(
      accountsFor(provider).filter(
        (account) =>
          (requestedAt.current.get(accountUsageKey(account)) ?? 0) < cutoff,
      ),
    );
  }, [accountsVersion, enabled, load, provider]);

  const refresh = useCallback(
    () => void load(accountsFor(provider)),
    [load, provider],
  );

  return { usage, now, refreshing, refresh };
}

export type AccountStatusTone =
  "ready" | "low" | "exhausted" | "checking" | "unknown";

export type AccountStatus = {
  tone: AccountStatusTone;
  label: string;
  /** Extra context, e.g. "back in 31m" for an exhausted account. */
  detail: string | null;
};

const LOW_HEADROOM_PERCENT = 20;

/** One-word readiness for an account, shared by Settings and the footer. */
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
          : "Usage unavailable",
      detail: null,
    };
  }
  if (headroom <= 0) {
    return { tone: "exhausted", label: "Exhausted", detail: backIn(limits, now) };
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

const STATUS_DOT: Record<AccountStatusTone, string> = {
  ready: "bg-emerald-400",
  low: "bg-amber-400",
  exhausted: "bg-red-400",
  checking: "animate-pulse bg-content/25",
  unknown: "bg-content/25",
};

const STATUS_TEXT: Record<AccountStatusTone, string> = {
  ready: "text-content/60",
  low: "text-amber-400/90",
  exhausted: "text-red-400/90",
  checking: "text-content/35",
  unknown: "text-content/35",
};

/** Dot + word, e.g. "● Ready" or "● Exhausted back in 31m". */
export function AccountStatusLabel({
  status,
  className = "",
}: {
  status: AccountStatus;
  className?: string;
}) {
  return (
    <span
      className={`inline-flex min-w-0 items-center gap-1.5 ${className}`}
      title={status.detail ? `${status.label} · ${status.detail}` : status.label}
    >
      <span
        className={`size-1.5 shrink-0 rounded-full ${STATUS_DOT[status.tone]}`}
        aria-hidden
      />
      <span className={`shrink-0 ${STATUS_TEXT[status.tone]}`}>
        {status.label}
      </span>
      {status.detail ? (
        <span className="min-w-0 truncate text-content/40">
          {status.detail}
        </span>
      ) : null}
    </span>
  );
}

/** Headroom for ranking accounts: the tightest window's remaining percent. */
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

export function AccountUsageRefresh({ usage }: { usage: AccountUsage }) {
  return (
    <button
      type="button"
      disabled={usage.refreshing}
      onClick={usage.refresh}
      aria-label="Refresh usage limits"
      title="Refresh usage limits"
      className="grid size-7 place-items-center rounded-md text-content/40 transition-transform duration-150 hover:bg-content/10 hover:text-content active:scale-[0.96] disabled:opacity-40"
    >
      <RefreshCw
        className={`size-3.5 ${usage.refreshing ? "animate-spin" : ""}`}
        strokeWidth={1.75}
      />
    </button>
  );
}

/** Compact 5h / weekly meters for one account row. */
export function AccountUsageMeters({
  limits,
  now,
}: {
  limits: ProviderRateLimits | undefined;
  now: number;
}) {
  const windows = [
    limits?.session ? { title: "5h", window: limits.session } : null,
    limits?.weekly ? { title: "Weekly", window: limits.weekly } : null,
    limits?.monthly ? { title: "Monthly", window: limits.monthly } : null,
  ].filter((entry) => entry != null);

  if (windows.length === 0) {
    const loading =
      !limits || limits.status === "idle" || limits.status === "fetching";
    return (
      <div className="hidden shrink-0 gap-4 sm:flex">
        {loading ? (
          <>
            <MeterSkeleton />
            <MeterSkeleton />
          </>
        ) : (
          // The row's status line already explains why there is no data.
          <span className="w-[19rem]" aria-hidden />
        )}
      </div>
    );
  }

  return (
    <div className="hidden shrink-0 gap-4 sm:flex">
      {windows.map((entry) => (
        <UsageMeter
          key={entry.title}
          title={entry.title}
          window={entry.window}
          now={now}
        />
      ))}
    </div>
  );
}

function UsageMeter({
  title,
  window,
  now,
}: {
  title: string;
  window: RateLimitWindow;
  now: number;
}) {
  const pct = clampUsedPercent(window.usedPercent);
  const full = pct >= 100 && (window.resetsAt == null || window.resetsAt > now);
  const reset =
    window.resetsAt == null
      ? formatWindowLabel(window.windowMinutes)
      : window.resetsAt <= now
        ? "reset due"
        : formatResetDuration(window.resetsAt - now);
  return (
    <div
      className="w-36"
      title={
        window.resetsAt == null
          ? undefined
          : `Resets ${new Date(window.resetsAt).toLocaleString()}`
      }
    >
      <div className="flex items-baseline justify-between gap-2 text-[10px] leading-3">
        <span className="min-w-0 truncate text-content/40">
          {title} · <span className="tabular-nums">{reset}</span>
        </span>
        <span
          className={`shrink-0 tabular-nums ${full ? "font-medium text-red-400" : "text-content/60"}`}
        >
          {full ? "Full" : formatUsagePercent(pct)}
        </span>
      </div>
      <div
        className="mt-1.5 h-1 overflow-hidden rounded-full bg-content/10"
        role="progressbar"
        aria-label={`${title} limit used`}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={Math.round(pct)}
      >
        <span
          className={`block h-full rounded-full transition-[width] duration-300 ${barClass(pct)}`}
          style={{ width: `${pct}%` }}
        />
      </div>
    </div>
  );
}

function MeterSkeleton() {
  return (
    <div className="w-36 animate-pulse" aria-hidden>
      <div className="h-3 w-20 rounded bg-content/10" />
      <div className="mt-1.5 h-1 rounded-full bg-content/10" />
    </div>
  );
}

/** Same thresholds as the footer usage chip. */
export function barClass(pct: number): string {
  if (pct >= 90) return "bg-red-400";
  if (pct >= 80) return "bg-amber-400";
  return "bg-content/45";
}
