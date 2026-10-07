import { useEffect, useRef, useState } from "react";
import { RefreshCw } from "../../shared/ui/icons";
import { HarnessIcon } from "../../features/sessions/ui/HarnessIcon";
import {
  fetchPiUsage,
  piBillingProvider,
  piUsageProvider,
  type PiUsageProvider,
} from "../../features/providers/model/piUsage";
import {
  idleRateLimits,
  RATE_LIMIT_MIN_REFETCH_MS,
  RATE_LIMIT_POLL_MS,
} from "../../features/providers/model/rateLimits";
import { UsageProviderChip } from "./UsageProviderChip";

export function PiUsage({ model, now }: { model?: string; now: number }) {
  const provider = piUsageProvider(model);
  if (!provider) {
    return (
      <span
        className="inline-flex items-center gap-1.5 whitespace-nowrap"
        title={
          !model || model === "pi:default"
            ? "Send a message so Pi can report its configured provider."
            : "Subscription usage is not supported for this Pi provider."
        }
      >
        <HarnessIcon harness="pi" className="size-3 shrink-0" />
        <span>pi · Usage unavailable</span>
      </span>
    );
  }
  return <PiProviderUsage key={provider} provider={provider} now={now} />;
}

function PiProviderUsage({
  provider,
  now,
}: {
  provider: PiUsageProvider;
  now: number;
}) {
  const [limits, setLimits] = useState(() =>
    idleRateLimits(piBillingProvider(provider)),
  );
  const refreshRef = useRef<(force?: boolean) => void>(() => undefined);
  useEffect(() => {
    let disposed = false;
    let inflight = false;
    let lastFetchAt = 0;
    const refresh = (force = false) => {
      if (disposed || inflight) return;
      if (
        !force &&
        (document.visibilityState !== "visible" ||
          Date.now() - lastFetchAt < RATE_LIMIT_MIN_REFETCH_MS)
      )
        return;
      inflight = true;
      setLimits({
        ...idleRateLimits(piBillingProvider(provider)),
        status: "fetching",
      });
      void fetchPiUsage(provider)
        .then((result) => {
          if (!disposed) setLimits(result);
        })
        .finally(() => {
          inflight = false;
          lastFetchAt = Date.now();
        });
    };
    refreshRef.current = refresh;
    refresh();
    const timer = window.setInterval(refresh, RATE_LIMIT_POLL_MS);
    const onVisible = () => refresh();
    document.addEventListener("visibilitychange", onVisible);
    window.addEventListener("focus", onVisible);
    return () => {
      disposed = true;
      window.clearInterval(timer);
      document.removeEventListener("visibilitychange", onVisible);
      window.removeEventListener("focus", onVisible);
    };
  }, [provider]);
  const fetching = limits.status === "fetching";
  return (
    <>
      <UsageProviderChip
        limits={limits}
        now={now}
        presentation={{
          sourceLabel: "Pi's saved OAuth account",
          harness: "pi",
          label:
            provider === "anthropic" ? "Pi · Anthropic" : "Pi · OpenAI Codex",
        }}
      />
      <button
        type="button"
        aria-label="Refresh Pi usage"
        title="Refresh Pi usage"
        disabled={fetching}
        onClick={() => refreshRef.current(true)}
        className="grid size-6 shrink-0 place-items-center rounded text-content/40 hover:bg-content/10 hover:text-content disabled:opacity-50 focus-visible:outline-2 focus-visible:outline-accent"
      >
        <RefreshCw
          className={`size-2.5 ${fetching ? "motion-safe:animate-spin" : ""}`}
          strokeWidth={1.75}
          aria-hidden
        />
      </button>
    </>
  );
}
