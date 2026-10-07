import { useEffect, useState, useSyncExternalStore } from "react";
import type { Session } from "../../sessions/model/session";
import { usageSnapshot } from "../../agent-app/model/usageSnapshot";
import { ChevronDown } from "../../../shared/ui/icons";
import {
  getAllRateLimits,
  subscribeRateLimits,
} from "../../providers/model/rateLimitsCache";

const MINUTE = 60_000;

/** Compact, read-only allowance view backed by the same cache as app usage.list. */
export function MonoUsage({ session }: { session: Session }) {
  useSyncExternalStore(subscribeRateLimits, getAllRateLimits, getAllRateLimits);
  // Freshness can change without another cache write.
  const [, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), MINUTE);
    return () => window.clearInterval(timer);
  }, []);
  const accounts = usageSnapshot(session, {}).accounts;
  const counts = accounts.reduce<Record<string, number>>((result, account) => {
    const status =
      account.status === "ok" && account.stale ? "stale" : account.status;
    result[status] = (result[status] ?? 0) + 1;
    return result;
  }, {});
  const label = Object.entries(counts)
    .map(([status, count]) => `${count} ${statusLabel(status).toLowerCase()}`)
    .join(", ");

  return (
    <details
      data-mono-usage
      className="group mx-3 mb-2 shrink-0 rounded-lg border border-content/8 bg-content/3 text-[11px] text-content/65"
      onToggle={(event) => {
        if (event.currentTarget.open)
          void usageSnapshot(session, { refresh: true });
      }}
    >
      <summary className="cursor-pointer list-none px-2.5 py-1.5 hover:text-content">
        <span className="font-medium text-content/80">Provider usage</span>
        <span className="ml-2">{label || "Checking…"}</span>
        <ChevronDown className="ml-1 inline size-3 transition-transform group-open:rotate-180" />
      </summary>
      <div className="max-h-64 space-y-2 overflow-y-auto border-t border-content/8 px-2.5 py-2">
        <div className="flex items-center justify-between gap-2">
          <span>Cached allowance snapshot</span>
          <button
            type="button"
            className="text-accent hover:underline"
            onClick={() => void usageSnapshot(session, { refresh: true })}
          >
            Refresh
          </button>
        </div>
        {accounts.map((account) => (
          <section
            key={`${account.provider}:${account.accountId}`}
            className="border-t border-content/6 pt-2 first:border-0 first:pt-0"
          >
            <div className="flex flex-wrap items-baseline justify-between gap-x-2">
              <h3 className="font-medium text-content">
                {providerLabel(account.provider)} · {account.accountLabel} (
                {account.accountId})
                {account.selectedForProject ? " · project account" : ""}
                {account.selectedForSession ? " · this conversation" : ""}
              </h3>
              <span>{accountStatusLabel(account)}</span>
            </div>
            {account.cliAvailable === false &&
            account.status !== "unsupported" ? (
              <p>Provider CLI not detected.</p>
            ) : null}
            {account.windows.length ? (
              <ul className="mt-1 space-y-0.5">
                {account.windows.map((window) => (
                  <li key={`${window.id}:${window.scope}`}>
                    {window.scope === "account" ? "Shared" : window.scope}
                    {window.id ? ` · ${window.id}` : ""}
                    {window.windowMinutes != null
                      ? ` · ${windowDuration(window.windowMinutes)}`
                      : ""}
                    {`: ${window.usedPercent}% used, ${window.remainingPercent}% remaining`}
                    {window.resetsAt != null
                      ? ` · resets ${dateTime(window.resetsAt)}`
                      : " · reset time unavailable"}
                  </li>
                ))}
              </ul>
            ) : null}
            {account.extraUsage ? (
              <p className="mt-1">
                Extra usage:{" "}
                {account.extraUsage.usedPercent != null
                  ? `${account.extraUsage.usedPercent}% used`
                  : account.extraUsage.usedCredits != null
                    ? `${account.extraUsage.usedCredits} ${account.extraUsage.currency ?? "credits"} used`
                    : "usage amount unavailable"}
                {account.extraUsage.monthlyLimit != null
                  ? ` of ${account.extraUsage.monthlyLimit} ${account.extraUsage.currency ?? "credits"}`
                  : " · limit unavailable"}
              </p>
            ) : null}
            {account.credits?.map((credit) => (
              <p key={credit.scope} className="mt-1">
                {credit.scope} credits:{" "}
                {credit.unlimited === true
                  ? "unlimited"
                  : credit.balance != null
                    ? credit.balance
                    : credit.hasCredits === false
                      ? "none"
                      : "balance unavailable"}
              </p>
            ))}
            {account.resetCredits ? (
              <p className="mt-1">
                Reset credits: {account.resetCredits.availableCount} available
                {account.resetCredits.credits?.length
                  ? ` · ${account.resetCredits.credits
                      .map((credit) =>
                        credit.expiresAt == null
                          ? credit.status
                          : `${credit.status}, expires ${dateTime(credit.expiresAt)}`,
                      )
                      .join("; ")}`
                  : ""}
              </p>
            ) : null}
            {account.fetchedAt != null ? (
              <p className="mt-1">
                Last successful snapshot: {dateTime(account.fetchedAt)}
              </p>
            ) : account.updatedAt != null && account.updatedAt > 0 ? (
              <p className="mt-1">
                Last checked: {dateTime(account.updatedAt)}
              </p>
            ) : null}
          </section>
        ))}
      </div>
    </details>
  );
}

function statusLabel(status: string): string {
  return (
    {
      ok: "Current",
      stale: "Stale snapshot",
      loading: "Loading",
      unavailable: "Unavailable",
      unsupported: "Unsupported",
      "authentication-error": "Authentication error",
      "fetch-error": "Fetch error",
    }[status] ?? status
  );
}

function accountStatusLabel(account: {
  status: string;
  stale: boolean;
}): string {
  const status =
    account.status === "ok" && account.stale ? "stale" : account.status;
  return `${statusLabel(status)}${account.stale && status !== "stale" ? " · stale snapshot" : ""}`;
}

function providerLabel(provider: string): string {
  return (
    {
      claude: "Claude Code",
      codex: "Codex",
      opencode: "OpenCode Go",
      pi: "Pi",
    }[provider] ?? provider
  );
}

function windowDuration(minutes: number): string {
  if (minutes % 1440 === 0) return `${minutes / 1440}d`;
  if (minutes % 60 === 0) return `${minutes / 60}h`;
  return `${minutes}m`;
}

function dateTime(timestamp: number): string {
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(timestamp);
}
