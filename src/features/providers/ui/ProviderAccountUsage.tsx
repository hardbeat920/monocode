import {
  clampUsedPercent,
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

/** Bar colour by percent used; shared with the footer usage chip. */
export function barClass(pct: number): string {
  if (pct >= 90) return "bg-red-400";
  if (pct >= 80) return "bg-amber-400";
  return "bg-content/45";
}
