import { useState } from "react";
import { HARNESS_TITLE, type HarnessId } from "../model/session";
import { HarnessIcon } from "./HarnessIcon";
import type { ProviderSignInState } from "./ProviderSignInPanel";
import { RefreshCw, RotateCcw } from "../../../shared/ui/icons";

const BUTTON =
  "flex h-6.5 shrink-0 items-center gap-1.5 rounded-md bg-selection px-2 text-[11px] text-content hover:bg-selection-hover disabled:cursor-default disabled:opacity-55";

/**
 * Offered under a turn that failed because the provider's sign-in expired or
 * is missing. Signing in runs the same flow as the usage chip; once it
 * succeeds the failed message can be sent again.
 */
export function ProviderSignInNotice({
  harness,
  message,
  onSignIn,
  onRetry,
}: {
  harness: HarnessId;
  /** The provider's own error, kept for reference. */
  message: string;
  onSignIn: () => Promise<void>;
  onRetry?: () => void;
}) {
  const [state, setState] = useState<ProviderSignInState>("idle");
  const [error, setError] = useState<string | null>(null);
  const title = HARNESS_TITLE[harness];
  const complete = state === "complete";

  const signIn = () => {
    setState("running");
    setError(null);
    onSignIn().then(
      () => setState("complete"),
      (reason: unknown) => {
        setError(
          reason instanceof Error
            ? reason.message
            : `Could not sign in to ${title}.`,
        );
        setState("error");
      },
    );
  };

  return (
    <div className="px-4 pt-1 pb-3" data-provider-sign-in>
      <div
        className="rounded-lg border border-amber-400/20 bg-amber-400/6 px-3 py-2.5 text-[12px] text-content/60"
        role="status"
        aria-live="polite"
      >
        <div className="flex items-center gap-2">
          <HarnessIcon harness={harness} className="size-3.5 shrink-0" />
          <span className="min-w-0 text-content/85">
            {complete
              ? `Signed in to ${title}`
              : `Sign in to ${title} to continue`}
          </span>
        </div>
        <div className="pl-5.5">
          <p className="mt-1 whitespace-pre-line break-words leading-relaxed text-content/45">
            {message}
          </p>
          {state === "error" && error ? (
            <p className="mt-1 leading-relaxed text-red-500">{error}</p>
          ) : null}
          <div className="mt-2 flex flex-wrap items-center gap-x-3 gap-y-1.5">
            {complete ? (
              onRetry ? (
                <button type="button" className={BUTTON} onClick={onRetry}>
                  <RotateCcw className="size-3.5" aria-hidden />
                  Retry
                </button>
              ) : (
                <span>Send your message again to continue.</span>
              )
            ) : (
              <button
                type="button"
                className={BUTTON}
                disabled={state === "running"}
                onClick={signIn}
              >
                {state === "running" ? (
                  <RefreshCw className="size-3.5 animate-spin" aria-hidden />
                ) : null}
                {state === "running" ? "Waiting for browser…" : "Sign in"}
              </button>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
