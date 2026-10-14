import {
  clampUsedPercent,
  DAILY_WINDOW_MINUTES,
  formatExtraUsageBalance,
  formatResetDuration,
  formatUsagePercent,
  formatWindowLabel,
  type ProviderRateLimits,
  type RateLimitWindow,
} from "../model/rateLimits";
import type {
  AccountStatus,
  AccountStatusTone,
  AccountUsage,
} from "../model/accountUsage";
import { RefreshCw } from "../../../shared/ui/icons";
import { getLocaleTag, t as translate } from "../../../shared/i18n/locale";
import { useT } from "../../../shared/i18n/useT";
import { useShowRemainingUsage } from "../../settings/model/displayPrefs";

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
      title={
        status.detail ? `${status.label} · ${status.detail}` : status.label
      }
    >
      <span
        className={`size-1.5 shrink-0 rounded-full ${STATUS_DOT[status.tone]}`}
        aria-hidden
      />
      <span
        className={`${status.tone === "unknown" ? "min-w-0 truncate" : "shrink-0"} ${STATUS_TEXT[status.tone]}`}
      >
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

export function AccountUsageRefresh({ usage }: { usage: AccountUsage }) {
  const t = useT();
  return (
    <button
      type="button"
      disabled={usage.refreshing}
      onClick={usage.refresh}
      aria-label={t("providers.account.refreshUsage")}
      title={t("providers.account.refreshUsage")}
      className="grid size-7 place-items-center rounded-md text-content/40 transition-transform duration-150 hover:bg-content/10 hover:text-content active:scale-[0.96] disabled:opacity-40"
    >
      <RefreshCw
        className={`size-3.5 ${usage.refreshing ? "animate-spin" : ""}`}
        strokeWidth={1.75}
      />
    </button>
  );
}

/** Titled 5h / weekly / monthly windows an account has data for. */
export function meterWindows(
  limits: ProviderRateLimits | undefined,
): { title: string; window: RateLimitWindow }[] {
  return [
    limits?.session
      ? {
          // Devin's short window is a day, not five hours.
          title:
            limits.session.windowMinutes === DAILY_WINDOW_MINUTES
              ? translate("providers.meter.daily")
              : translate("providers.meter.fiveHours"),
          window: limits.session,
        }
      : null,
    limits?.weekly
      ? { title: translate("providers.meter.weekly"), window: limits.weekly }
      : null,
    limits?.monthly
      ? { title: translate("providers.meter.monthly"), window: limits.monthly }
      : null,
  ].filter((entry) => entry != null);
}

/** Compact 5h / weekly meters for one account row. */
export function AccountUsageMeters({
  limits,
  now,
}: {
  limits: ProviderRateLimits | undefined;
  now: number;
}) {
  const t = useT();
  const windows = meterWindows(limits);

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
      {limits?.extraUsageBalance != null ? (
        <div
          className="w-16 text-[10px] leading-3"
          title={t("providers.account.extraBalance")}
        >
          <div className="truncate text-content/40">
            {t("providers.account.extra")}
          </div>
          <div className="mt-1 font-medium tabular-nums text-content/70">
            {formatExtraUsageBalance(limits.extraUsageBalance)}
          </div>
        </div>
      ) : null}
    </div>
  );
}

export function UsageMeter({
  title,
  window,
  now,
  className = "w-36",
}: {
  title: string;
  window: RateLimitWindow;
  now: number;
  className?: string;
}) {
  const t = useT();
  const showRemaining = useShowRemainingUsage();
  const pct = clampUsedPercent(window.usedPercent);
  const remaining = 100 - pct;
  const shown = showRemaining ? remaining : pct;
  const full = pct >= 100 && (window.resetsAt == null || window.resetsAt > now);
  const reset =
    window.resetsAt == null
      ? formatWindowLabel(window.windowMinutes)
      : window.resetsAt <= now
        ? t("providers.meter.resetDue")
        : formatResetDuration(window.resetsAt - now);
  return (
    <div
      className={className}
      title={
        window.resetsAt == null
          ? undefined
          : t("providers.meter.resets", {
              date: new Date(window.resetsAt).toLocaleString(getLocaleTag()),
            })
      }
    >
      <div className="flex items-baseline justify-between gap-2 text-[10px] leading-3">
        <span className="min-w-0 truncate text-content/40">
          {title} · <span className="tabular-nums">{reset}</span>
        </span>
        <span
          className={`shrink-0 tabular-nums ${full ? "font-medium text-red-400" : "text-content/60"}`}
        >
          {showRemaining
            ? t("providers.meter.left", {
                percent: formatUsagePercent(remaining),
              })
            : full
              ? t("providers.meter.full")
              : formatUsagePercent(pct)}
        </span>
      </div>
      <div
        className="mt-1.5 h-1 overflow-hidden rounded-full bg-content/10"
        role="progressbar"
        aria-label={t(
          showRemaining
            ? "providers.meter.limitRemaining"
            : "providers.meter.limitUsed",
          { title },
        )}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={Math.round(shown)}
      >
        <span
          className={`block h-full rounded-full transition-[width] duration-300 ${barClass(pct)}`}
          style={{ width: `${shown}%` }}
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

/** Bar colour by percent used; shared with the footer usage chip. */
export function barClass(pct: number): string {
  if (pct >= 90) return "bg-red-400";
  if (pct >= 80) return "bg-amber-400";
  return "bg-content/45";
}
