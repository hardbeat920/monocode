import {
  HARNESSES,
  type HarnessId,
  type Session,
} from "../../sessions/model/session";
import { isHarnessAvailable } from "../../../integrations/harness/core/availability";
import {
  providerAccounts,
  selectedProviderAccountId,
  supportsProviderAccounts,
} from "../../providers/model/providerAccounts";
import { piUsageProvider } from "../../providers/model/piUsage";
import {
  getCachedRateLimits,
  loadRateLimits,
  type RateLimitSource,
} from "../../providers/model/rateLimitsCache";
import { RATE_LIMIT_POLL_MS } from "../../providers/model/rateLimits";

type Target = {
  provider: HarnessId;
  accountId: string;
  accountLabel: string;
  selectedForProject: boolean | null;
  selectedForSession: boolean;
  source?: RateLimitSource;
  cacheAccountId: string;
  removed?: boolean;
};

/**
 * Return an allowlisted account/usage snapshot independently of the caller's harness.
 * Validate filters, preserve unknown/stale states, and never change account selection.
 * Cached reads do not probe; explicit refresh starts shared throttled loads and
 * returns immediately, so loading rows may need a subsequent cached read.
 */
export function usageSnapshot(source: Session, input: Record<string, unknown>) {
  const { provider, accountId, refresh = false } = input;
  if (provider !== undefined && !HARNESSES.includes(provider as HarnessId))
    throw new Error("Unknown provider; run models.list for provider IDs");
  if (
    accountId !== undefined &&
    (typeof accountId !== "string" || !/^[A-Za-z0-9_-]{1,80}$/.test(accountId))
  )
    throw new Error("accountId must be an account ID from usage.list");
  if (typeof refresh !== "boolean")
    throw new Error("refresh must be a boolean");
  const targets: Target[] = HARNESSES.flatMap((provider): Target[] => {
    if (supportsProviderAccounts(provider)) {
      const selected = selectedProviderAccountId(provider, source.cwd);
      const sessionId = source.providerAccountId ?? "default";
      const accounts = providerAccounts(provider);
      const rows: Target[] = accounts.map((account) => ({
        provider,
        accountId: account.id,
        accountLabel: account.label,
        selectedForProject: account.id === selected,
        selectedForSession:
          source.harness === provider && account.id === sessionId,
        source: provider,
        cacheAccountId: account.id,
      }));
      if (
        source.harness === provider &&
        !accounts.some((account) => account.id === sessionId)
      ) {
        rows.push({
          provider,
          accountId: sessionId,
          accountLabel: "Removed account",
          selectedForProject: false,
          selectedForSession: true,
          cacheAccountId: sessionId,
          removed: true,
        });
      }
      return rows;
    }
    if (provider === "pi") {
      return (["anthropic", "openai-codex"] as const).map((id) => ({
        provider,
        accountId: id,
        accountLabel: `Pi saved ${id} account`,
        selectedForProject: null,
        selectedForSession:
          source.harness === "pi" && piUsageProvider(source.model) === id,
        source: `pi:${id}`,
        cacheAccountId: "default",
      }));
    }
    return [
      {
        provider,
        accountId: "default",
        accountLabel: "Default account",
        selectedForProject: null,
        selectedForSession: source.harness === provider,
        source: provider === "opencode" ? provider : undefined,
        cacheAccountId: "default",
      },
    ];
  }).filter(
    (target) =>
      (provider === undefined || target.provider === provider) &&
      (accountId === undefined || target.accountId === accountId),
  );

  if (refresh) {
    for (const target of targets) {
      if (target.source) {
        // Return loading immediately: account probes are serialized by Codex and
        // can outlive the app RPC timeout. Subsequent cached reads see completion.
        void loadRateLimits(target.source, target.cacheAccountId, "throttled");
      }
    }
  }
  const snapshotAt = Date.now();
  return {
    snapshotAt,
    accounts: targets.map((target) => {
      const limits = target.source
        ? getCachedRateLimits(target.source, target.cacheAccountId)
        : null;
      const windows = (
        (limits?.windows?.length ? limits.windows : null) ??
        (["session", "weekly", "monthly"] as const).flatMap((id) => {
          const window = limits?.[id];
          return window ? [{ id, scope: "account", ...window }] : [];
        })
      ).map((window) => ({
        id: window.id,
        scope: window.scope,
        unit: "percent",
        usedPercent: window.usedPercent,
        remainingPercent: 100 - window.usedPercent,
        windowMinutes: window.windowMinutes,
        resetsAt: window.resetsAt,
      }));
      const fetchedAt =
        limits?.fetchedAt ??
        (limits?.status === "ok" ? limits.updatedAt : null);
      const hasData =
        windows.length > 0 ||
        !!limits?.extraUsage ||
        !!limits?.credits?.length ||
        !!limits?.resetCredits;
      const stale =
        hasData &&
        (limits?.status !== "ok" ||
          fetchedAt == null ||
          snapshotAt - fetchedAt >= RATE_LIMIT_POLL_MS ||
          windows.some(
            (window) =>
              window.resetsAt != null && window.resetsAt <= snapshotAt,
          ));
      // Classify internally; never return raw provider/transport error text.
      const authError =
        limits?.error &&
        /not signed in|sign.in expired|not authenticated|authentication required|sign in.*(?:provider|pi)|\b401\b/i.test(
          limits.error,
        );
      const status = !target.source
        ? target.removed
          ? "unavailable"
          : "unsupported"
        : limits?.status === "fetching"
          ? "loading"
          : authError
            ? "authentication-error"
            : limits?.status === "error"
              ? "fetch-error"
              : limits?.status !== "ok" || !hasData
                ? "unavailable"
                : stale
                  ? "stale"
                  : "ok";
      const extra = limits?.extraUsage;
      return {
        provider: target.provider,
        accountId: target.accountId,
        accountLabel: target.accountLabel,
        selectedForProject: target.selectedForProject,
        selectedForSession: target.selectedForSession,
        cliAvailable: isHarnessAvailable(target.provider),
        status,
        stale,
        fetchedAt,
        updatedAt: limits?.updatedAt || null,
        windows,
        extraUsage: extra
          ? {
              enabled: extra.enabled,
              monthlyLimit: extra.monthlyLimit,
              usedCredits: extra.usedCredits,
              usedPercent: extra.usedPercent,
              currency: extra.currency,
              unit: "provider_credits",
            }
          : null,
        credits: (limits?.credits ?? []).map((credit) => ({
          scope: credit.scope,
          hasCredits: credit.hasCredits,
          unlimited: credit.unlimited,
          balance: credit.balance,
          unit: "credits",
        })),
        resetCredits: limits?.resetCredits
          ? {
              availableCount: limits.resetCredits.availableCount,
              credits:
                limits.resetCredits.credits?.map((credit) => ({
                  status: credit.status,
                  resetType: credit.resetType,
                  grantedAt: credit.grantedAt,
                  expiresAt: credit.expiresAt,
                })) ?? null,
            }
          : null,
      };
    }),
  };
}
