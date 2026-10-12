import { useEffect, useState, type ReactNode } from "react";
import { Clock, Gauge, Play, X } from "../../../shared/ui/icons";
import type { UsageLimit } from "../model/session";
import { formatUsageLimitReset } from "../model/usageLimit";

const BUTTON =
  "flex h-6 shrink-0 items-center gap-1.5 rounded-md px-1.5 hover:bg-content/10 hover:text-content";

export function UsageLimitNotice({
  limit,
  onResume,
  onResumeAtReset,
  onDismiss,
  variant = "composer",
  providerName,
  modelPicker,
}: {
  limit: UsageLimit;
  onResume?: () => void;
  onResumeAtReset?: (enabled: boolean) => void;
  onDismiss?: () => void;
  variant?: "composer" | "mono";
  providerName?: string;
  modelPicker?: ReactNode;
}) {
  const [now, setNow] = useState(Date.now);
  const waiting = limit.resetsAt != null && limit.resetsAt > now;
  // Tick the countdown, and flip to "Resume" once the window resets.
  useEffect(() => {
    if (!waiting) return;
    const timer = window.setInterval(() => setNow(Date.now()), 30_000);
    return () => window.clearInterval(timer);
  }, [waiting]);

  return (
    <div
      className={
        variant === "mono"
          ? "mx-1.5 mb-2 rounded-lg border border-amber-400/20 bg-amber-400/6 px-3 py-2.5 text-[12px] text-content/60"
          : "px-2 text-content/55"
      }
      data-usage-limit
      role="status"
    >
      <div
        className={
          variant === "mono"
            ? "flex items-center gap-2"
            : "relative z-0 flex h-8 items-center gap-2 rounded-t-[10px] border border-b-0 border-amber-400/25 bg-amber-400/10 px-2 text-[12px]"
        }
      >
        <Gauge className="size-3.5 shrink-0 text-amber-400" />
        <span className="shrink-0 text-content/85">
          {providerName
            ? `${providerName} usage limit reached`
            : "Usage limit reached"}
        </span>
        {variant === "mono" ? null : (
          <span className="min-w-0 flex-1 truncate">
            {limit.resetsAt == null
              ? ""
              : waiting
                ? `Resets ${formatUsageLimitReset(limit.resetsAt, now)}`
                : "Limit has reset"}
          </span>
        )}
        {variant === "mono" ? null : modelPicker}
        {variant === "mono" ? null : (
          <ResumeControls
            limit={limit}
            waiting={waiting}
            onResume={onResume}
            onResumeAtReset={onResumeAtReset}
          />
        )}
        {onDismiss ? (
          <button
            type="button"
            title="Dismiss"
            aria-label="Dismiss usage limit notice"
            onClick={onDismiss}
            className="ml-auto grid size-6 shrink-0 place-items-center rounded-md hover:bg-content/10 hover:text-content"
          >
            <X className="size-3.5" />
          </button>
        ) : null}
      </div>
      {variant === "mono" ? (
        <div className="pl-5.5">
          <p className="mt-1 leading-relaxed">
            Your conversation is saved. Choose another model or account to keep
            going.
          </p>
          {limit.resetsAt != null ? (
            <p className="mt-1 text-content/45">
              {waiting
                ? `Resets ${formatUsageLimitReset(limit.resetsAt, now)}`
                : "Limit has reset"}
            </p>
          ) : null}
          <div className="mt-2 flex flex-wrap items-center gap-x-3 gap-y-1.5">
            {modelPicker}
            <ResumeControls
              limit={limit}
              waiting={waiting}
              onResume={onResume}
              onResumeAtReset={onResumeAtReset}
            />
          </div>
        </div>
      ) : null}
    </div>
  );
}

function ResumeControls({
  limit,
  waiting,
  onResume,
  onResumeAtReset,
}: {
  limit: UsageLimit;
  waiting: boolean;
  onResume?: () => void;
  onResumeAtReset?: (enabled: boolean) => void;
}) {
  return (
    <>
      {!waiting ? (
        <button type="button" onClick={onResume} className={BUTTON}>
          <Play className="size-3.5" />
          Resume
        </button>
      ) : limit.resumeAtReset ? (
        <button
          type="button"
          title="Cancel the automatic resume"
          onClick={() => onResumeAtReset?.(false)}
          className={`${BUTTON} text-amber-400`}
        >
          <Clock className="size-3.5" />
          Resuming at reset
        </button>
      ) : (
        <button
          type="button"
          title="Continue this session once the limit resets"
          onClick={() => onResumeAtReset?.(true)}
          className={BUTTON}
        >
          <Clock className="size-3.5" />
          Resume at reset
        </button>
      )}
    </>
  );
}
