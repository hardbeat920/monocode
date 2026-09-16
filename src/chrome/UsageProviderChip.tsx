import { useEffect, useRef, useState } from "react";
import {
  clampUsedPercent,
  formatRateLimitWindowChipLabel,
  formatResetCountdown,
  formatResetDuration,
  formatUsagePercent,
  formatWindowLabel,
  rateLimitWindowTooltip,
  type ProviderRateLimits,
  type RateLimitResetCredit,
  type RateLimitWindow,
} from "../lib/rateLimits";
import type { CodexRateLimitResetOutcome } from "../lib/rateLimitsFetch";
import { HARNESS_TITLE } from "../lib/session";
import { HarnessIcon } from "./HarnessIcon";
import { RefreshCw } from "./icons";
import { Popover, type PopoverDismissReason } from "./Popover";

type UsageWindowEntry = {
  key: "session" | "weekly";
  window: RateLimitWindow;
};

type ResetActionState =
  "idle" | "confirming" | "using" | CodexRateLimitResetOutcome | "error";

export function UsageProviderChip({
  limits,
  now,
  onConsumeReset,
}: {
  limits: ProviderRateLimits;
  now: number;
  onConsumeReset?: (creditId?: string) => Promise<CodexRateLimitResetOutcome>;
}) {
  const trigger = useRef<HTMLButtonElement>(null);
  const [open, setOpen] = useState(false);
  const [resetAction, setResetAction] = useState<ResetActionState>("idle");
  const [activeResetKey, setActiveResetKey] = useState<string | null>(null);
  const [resetError, setResetError] = useState<string | null>(null);
  const loading =
    limits.status === "idle" ||
    (limits.status === "fetching" && !limits.session && !limits.weekly);
  const disconnected = limits.status === "unavailable";
  const windows = usageWindows(limits);
  const tightest = windows.reduce<RateLimitWindow | null>((best, entry) => {
    if (!best || entry.window.usedPercent > best.usedPercent) {
      return entry.window;
    }
    return best;
  }, null);
  const tooltip = windows
    .map((entry) => rateLimitWindowTooltip(entry.window, now))
    .join(" · ");
  const providerLabel = HARNESS_TITLE[limits.provider];

  useEffect(() => {
    if (open) return;
    setResetAction("idle");
    setActiveResetKey(null);
    setResetError(null);
  }, [open]);

  const dismiss = (reason: PopoverDismissReason) => {
    setOpen(false);
    if (reason === "escape") {
      requestAnimationFrame(() => trigger.current?.focus());
    }
  };

  const useReset = async (
    credit: RateLimitResetCredit | undefined,
    rowKey: string,
  ) => {
    if (!onConsumeReset) return;
    setActiveResetKey(rowKey);
    setResetAction("using");
    setResetError(null);
    try {
      setResetAction(await onConsumeReset(credit?.id));
    } catch (error) {
      setResetError(
        error instanceof Error ? error.message : "Could not use this reset",
      );
      setResetAction("error");
    }
  };

  return (
    <>
      <button
        ref={trigger}
        type="button"
        className="-mx-1 inline-flex h-5 min-w-0 shrink-0 items-center gap-1.5 whitespace-nowrap rounded px-1 text-content/55 transition-[background-color,color,transform] duration-150 ease-out hover:bg-content/10 hover:text-content active:scale-[0.97]"
        aria-label={`${providerLabel} usage details`}
        aria-expanded={open}
        aria-haspopup="dialog"
        title={
          tooltip ||
          limits.error ||
          (disconnected
            ? "Not connected"
            : loading
              ? "Loading usage…"
              : "Usage details")
        }
        onClick={() => setOpen((value) => !value)}
      >
        <HarnessIcon harness={limits.provider} className="size-3 shrink-0" />
        {loading ? (
          <span className="animate-pulse text-content/35">···</span>
        ) : disconnected ? (
          <span className="text-content/35">not connected</span>
        ) : windows.length === 0 ? (
          <span className="text-content/35">{emptyUsageLabel(limits)}</span>
        ) : (
          <>
            {tightest ? <MiniBar usedPct={tightest.usedPercent} /> : null}
            <span className="flex min-w-0 items-center gap-1 tabular-nums">
              {windows.map((entry, index) => (
                <span
                  key={entry.key}
                  className="inline-flex items-center gap-1"
                >
                  {index > 0 ? (
                    <span className="text-content/25">·</span>
                  ) : null}
                  <span>
                    {formatUsagePercent(entry.window.usedPercent)}{" "}
                    {formatRateLimitWindowChipLabel(entry.window, now)}
                  </span>
                </span>
              ))}
            </span>
          </>
        )}
      </button>
      {open ? (
        <Popover
          anchor={trigger}
          side="top"
          align="start"
          gap={7}
          width={300}
          maxHeight={460}
          autoFocus
          onDismiss={dismiss}
          role="dialog"
          aria-label={`${providerLabel} usage details`}
          tabIndex={-1}
          className="overflow-y-auto p-2.5 text-content"
        >
          <div className="flex items-start gap-2.5 px-1 pb-2.5 pt-0.5">
            <span className="grid size-7 shrink-0 place-items-center rounded-lg bg-content/[0.06] ring-1 ring-inset ring-content/[0.07]">
              <HarnessIcon harness={limits.provider} className="size-4" />
            </span>
            <div className="min-w-0 flex-1">
              <h2 className="text-[13px] font-medium leading-4">
                {providerLabel} usage
              </h2>
              <p className="mt-0.5 text-[10px] leading-4 text-content/40">
                {updatedLabel(limits, now)}
              </p>
            </div>
            {limits.status === "fetching" ? (
              <span className="mt-0.5 inline-flex items-center gap-1 text-[10px] text-content/40">
                <RefreshCw
                  className="size-2.5 animate-spin"
                  strokeWidth={1.75}
                  aria-hidden
                />
                Updating
              </span>
            ) : null}
          </div>

          {limits.status === "error" && windows.length > 0 ? (
            <p className="mb-2 rounded-lg bg-amber-400/10 px-2.5 py-2 text-[10px] leading-4 text-amber-700 dark:text-amber-300">
              Couldn’t refresh. Showing the last available snapshot.
            </p>
          ) : null}

          {windows.length > 0 ? (
            <div className="flex flex-col gap-1.5">
              {windows.map((entry) => (
                <UsageWindowCard
                  key={entry.key}
                  kind={entry.key}
                  window={entry.window}
                  now={now}
                />
              ))}
            </div>
          ) : (
            <EmptyUsageState limits={limits} loading={loading} />
          )}

          {limits.provider === "codex" ? (
            <BankedResets
              limits={limits}
              now={now}
              action={resetAction}
              activeResetKey={activeResetKey}
              error={resetError}
              onConfirm={(creditId) => {
                setActiveResetKey(creditId);
                setResetAction("confirming");
              }}
              onCancel={() => {
                setActiveResetKey(null);
                setResetAction("idle");
              }}
              onUse={useReset}
              canUse={Boolean(onConsumeReset)}
            />
          ) : null}
        </Popover>
      ) : null}
    </>
  );
}

function usageWindows(limits: ProviderRateLimits): UsageWindowEntry[] {
  return [
    limits.session
      ? ({ key: "session", window: limits.session } as const)
      : null,
    limits.weekly ? ({ key: "weekly", window: limits.weekly } as const) : null,
  ].filter((entry): entry is UsageWindowEntry => entry != null);
}

function UsageWindowCard({
  kind,
  window,
  now,
}: {
  kind: UsageWindowEntry["key"];
  window: RateLimitWindow;
  now: number;
}) {
  const pct = clampUsedPercent(window.usedPercent);
  const remaining = Math.max(0, Math.round(100 - pct));
  const title =
    kind === "session"
      ? "5-hour limit"
      : kind === "weekly"
        ? "Weekly limit"
        : `${formatWindowLabel(window.windowMinutes)} limit`;
  return (
    <section className="rounded-lg bg-content/[0.045] px-3 py-2.5 ring-1 ring-inset ring-content/[0.06]">
      <div className="flex items-baseline justify-between gap-3">
        <h3 className="text-[11px] font-medium text-content/65">{title}</h3>
        <span className="shrink-0 text-[11px] font-medium tabular-nums">
          {formatUsagePercent(pct)} used
        </span>
      </div>
      <div
        className="mt-2 h-1.5 overflow-hidden rounded-full bg-content/10"
        role="progressbar"
        aria-label={`${title} used`}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={Math.round(pct)}
      >
        <span
          className={`block h-full rounded-full ${barClass(pct)}`}
          style={{ width: `${pct}%` }}
        />
      </div>
      <div className="mt-1.5 flex items-center justify-between gap-3 text-[10px] leading-4 text-content/40">
        <span className="tabular-nums">{remaining}% remaining</span>
        <span
          className="truncate text-right tabular-nums"
          title={
            window.resetsAt == null
              ? undefined
              : new Date(window.resetsAt).toLocaleString()
          }
        >
          {window.resetsAt == null
            ? `${formatWindowLabel(window.windowMinutes)} window`
            : formatResetCountdown(window.resetsAt - now)}
        </span>
      </div>
    </section>
  );
}

function BankedResets({
  limits,
  now,
  action,
  activeResetKey,
  error,
  onConfirm,
  onCancel,
  onUse,
  canUse,
}: {
  limits: ProviderRateLimits;
  now: number;
  action: ResetActionState;
  activeResetKey: string | null;
  error: string | null;
  onConfirm: (rowKey: string) => void;
  onCancel: () => void;
  onUse: (credit: RateLimitResetCredit | undefined, rowKey: string) => void;
  canUse: boolean;
}) {
  const summary = limits.resetCredits;
  const count = summary?.availableCount ?? null;
  const detailedCredits = (summary?.credits ?? []).filter(
    (credit) => credit.status === "available" || credit.status === "unknown",
  );
  const unlistedCount = Math.max(0, (count ?? 0) - detailedCredits.length);
  const rows: Array<RateLimitResetCredit | null> = [
    ...detailedCredits,
    ...Array.from({ length: unlistedCount }, () => null),
  ];
  return (
    <section className="mt-2.5 border-t border-content/[0.08] px-1 pt-2.5">
      <div className="flex items-start gap-2">
        <div className="min-w-0 flex-1">
          <h3 className="text-[11px] font-medium">Banked resets</h3>
          <p className="mt-0.5 text-[10px] leading-4 text-content/40">
            {count == null
              ? "Not reported by this account"
              : count === 0
                ? "No resets available"
                : `${count} ${count === 1 ? "reset" : "resets"} available`}
          </p>
        </div>
        {count != null ? (
          <span className="rounded-full bg-content/[0.07] px-2 py-0.5 text-[10px] font-medium tabular-nums text-content/65 ring-1 ring-inset ring-content/[0.07]">
            {count}
          </span>
        ) : null}
      </div>

      {count != null && count > 0 ? (
        <div
          className="mt-2 max-h-56 overflow-y-auto overscroll-contain pr-1 [scrollbar-gutter:stable]"
          aria-label="Available banked resets"
        >
          <div className="flex flex-col gap-1.5">
            {rows.map((credit, index) => {
              const rowKey = credit?.id ?? `unlisted-${index}`;
              const selected = activeResetKey === rowKey;
              return (
                <BankedResetRow
                  key={rowKey}
                  credit={credit}
                  index={index}
                  now={now}
                  action={selected ? action : "idle"}
                  error={selected ? error : null}
                  disabled={action === "using" && !selected}
                  canUse={canUse}
                  onConfirm={() => onConfirm(rowKey)}
                  onCancel={onCancel}
                  onUse={() => onUse(credit ?? undefined, rowKey)}
                />
              );
            })}
          </div>
        </div>
      ) : null}
    </section>
  );
}

function BankedResetRow({
  credit,
  index,
  now,
  action,
  error,
  disabled,
  canUse,
  onConfirm,
  onCancel,
  onUse,
}: {
  credit: RateLimitResetCredit | null;
  index: number;
  now: number;
  action: ResetActionState;
  error: string | null;
  disabled: boolean;
  canUse: boolean;
  onConfirm: () => void;
  onCancel: () => void;
  onUse: () => void;
}) {
  return (
    <article className="rounded-lg bg-content/[0.04] px-2.5 py-2 ring-1 ring-inset ring-content/[0.06]">
      <h4 className="text-[10px] font-medium leading-4 text-content/70">
        {credit?.title ?? `Banked reset ${index + 1}`}
      </h4>
      {credit?.description ? (
        <p className="mt-0.5 text-[10px] leading-4 text-content/45">
          {credit.description}
        </p>
      ) : null}
      <div className="mt-1.5 flex min-h-6 items-center justify-between gap-2">
        <p
          className="min-w-0 truncate text-[10px] tabular-nums text-content/40"
          title={
            credit?.expiresAt == null
              ? undefined
              : new Date(credit.expiresAt).toLocaleString()
          }
        >
          {credit?.expiresAt == null
            ? "Expiry not provided"
            : credit.expiresAt <= now
              ? "Expires now"
              : `Expires in ${formatResetDuration(credit.expiresAt - now)}`}
        </p>
        {action === "using" ? (
          <span className="inline-flex shrink-0 items-center gap-1.5 text-[10px] text-content/45">
            <RefreshCw
              className="size-3 animate-spin"
              strokeWidth={1.75}
              aria-hidden
            />
            Applying…
          </span>
        ) : isResetOutcome(action) || action === "error" ? (
          <span
            className={`shrink-0 text-[10px] ${
              action === "reset"
                ? "text-emerald-700 dark:text-emerald-300"
                : "text-content/50"
            }`}
            role="status"
          >
            {action === "error" ? error : resetOutcomeLabel(action)}
          </span>
        ) : canUse && action !== "confirming" ? (
          <button
            type="button"
            className="h-6 shrink-0 rounded-md bg-content/[0.07] px-2.5 text-[10px] font-medium text-content/70 ring-1 ring-inset ring-content/[0.08] transition-[background-color,color,transform] duration-150 ease-out hover:bg-content/[0.11] hover:text-content active:scale-[0.97] disabled:pointer-events-none disabled:opacity-35"
            disabled={disabled}
            onClick={onConfirm}
          >
            Use reset
          </button>
        ) : null}
      </div>
      {action === "confirming" ? (
        <div className="mt-2 flex items-center justify-between gap-2 border-t border-content/[0.07] pt-2">
          <p className="text-[10px] leading-4 text-content/50">
            Spend this reset now?
          </p>
          <div className="flex shrink-0 gap-1">
            <button
              type="button"
              className="h-6 rounded-md px-2 text-[10px] text-content/50 hover:bg-content/10 hover:text-content"
              onClick={onCancel}
            >
              Cancel
            </button>
            <button
              type="button"
              className="h-6 rounded-md bg-content px-2.5 text-[10px] font-medium text-background-base transition-transform duration-150 ease-out active:scale-[0.97]"
              onClick={onUse}
            >
              Confirm
            </button>
          </div>
        </div>
      ) : null}
    </article>
  );
}

function EmptyUsageState({
  limits,
  loading,
}: {
  limits: ProviderRateLimits;
  loading: boolean;
}) {
  return (
    <div className="rounded-lg bg-content/[0.04] px-3 py-4 text-center ring-1 ring-inset ring-content/[0.06]">
      <p className="text-[11px] font-medium text-content/65">
        {loading
          ? "Loading usage…"
          : limits.status === "unavailable"
            ? "Not connected"
            : "Usage unavailable"}
      </p>
      {limits.error ? (
        <p className="mx-auto mt-1 max-w-[15rem] text-[10px] leading-4 text-content/40">
          {limits.error}
        </p>
      ) : null}
    </div>
  );
}

function updatedLabel(limits: ProviderRateLimits, now: number): string {
  if (limits.updatedAt <= 0) return "Rate-limit details";
  const elapsedMinutes = Math.max(
    0,
    Math.floor((now - limits.updatedAt) / 60_000),
  );
  if (elapsedMinutes === 0) return "Updated just now";
  if (elapsedMinutes < 60) return `Updated ${elapsedMinutes}m ago`;
  return `Updated ${Math.floor(elapsedMinutes / 60)}h ago`;
}

function resetOutcomeLabel(outcome: CodexRateLimitResetOutcome): string {
  if (outcome === "reset") return "Codex usage was reset.";
  if (outcome === "nothingToReset") return "There’s no active usage to reset.";
  if (outcome === "noCredit") return "No banked resets are available.";
  return "That reset was already used.";
}

function isResetOutcome(
  value: ResetActionState,
): value is CodexRateLimitResetOutcome {
  return (
    value === "reset" ||
    value === "nothingToReset" ||
    value === "noCredit" ||
    value === "alreadyRedeemed"
  );
}

function emptyUsageLabel(limits: ProviderRateLimits): string {
  if (limits.status !== "error") return "—";
  const text = limits.error?.toLowerCase() ?? "";
  if (text.includes("expired") || text.includes("sign-in")) return "expired";
  return "—";
}

function MiniBar({ usedPct }: { usedPct: number }) {
  const pct = clampUsedPercent(usedPct);
  return (
    <span
      className="h-1 w-8 shrink-0 overflow-hidden rounded-full bg-content/10"
      aria-hidden
    >
      <span
        className={`block h-full rounded-full ${barClass(pct)}`}
        style={{ width: `${pct}%` }}
      />
    </span>
  );
}

function barClass(pct: number): string {
  if (pct >= 90) return "bg-red-400";
  if (pct >= 80) return "bg-amber-400";
  return "bg-content/45";
}
