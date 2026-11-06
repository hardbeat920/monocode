import type { HarnessId } from "../model/session";
import { HARNESS_TITLE } from "../model/session";
import { HarnessIcon } from "./HarnessIcon";
import { Check, RefreshCw } from "../../../shared/ui/icons";

export type ProviderSignInState =
  "idle" | "running" | "cancelling" | "complete" | "error";

export function ProviderSignInPanel({
  harness,
  state,
  error,
  onSignIn,
  onComplete,
  completeActionLabel,
  autoFocus = false,
  onCancel,
  cancelLabel,
  resumable = false,
  loginUrl,
  onCopyUrl,
  onSubmitCode,
}: {
  harness: HarnessId;
  state: ProviderSignInState;
  error: string | null;
  onSignIn: () => void;
  onComplete?: () => void;
  completeActionLabel?: string;
  autoFocus?: boolean;
  onCancel?: () => void;
  cancelLabel?: string;
  resumable?: boolean;
  loginUrl?: string | null;
  onCopyUrl?: () => void;
  onSubmitCode?: (code: string) => void;
}) {
  const title = HARNESS_TITLE[harness];
  const complete = state === "complete";

  return (
    <div
      className="flex min-h-68 flex-col items-center justify-center px-5 py-6"
      aria-live="polite"
    >
      <span className="grid size-16 place-items-center rounded-2xl bg-content/[0.06] text-content ring-1 ring-inset ring-content/[0.08] shadow-sm">
        <HarnessIcon harness={harness} className="size-9" />
      </span>
      <h2 className="mt-3.5 text-[15px] font-medium leading-5 text-content">
        {complete ? `Signed in to ${title}` : "Authentication required"}
      </h2>
      <p className="mt-1 max-w-56 text-[11px] leading-4 text-content/45">
        {complete
          ? "You can retry your last message now."
          : resumable
            ? "Your provider sign-in is still running. Continue it to use the same link."
            : `Sign in to continue using ${title}.`}
      </p>
      <button
        type="button"
        autoFocus={autoFocus}
        className="mt-4 inline-flex h-8 items-center  gap-1.5 rounded-lg bg-content px-3.5 text-[12px] font-medium text-background-base transition-transform duration-150 ease-out hover:bg-content/85 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent active:scale-[0.98] disabled:cursor-default disabled:opacity-55 text-center"
        disabled={
          state === "running" ||
          state === "cancelling" ||
          (complete && !onComplete)
        }
        onClick={complete && onComplete ? onComplete : onSignIn}
      >
        {state === "running" ? (
          <RefreshCw className="size-3.5 animate-spin" aria-hidden />
        ) : complete ? (
          <Check className="size-3.5" aria-hidden />
        ) : null}
        {state === "cancelling"
          ? "Cancelling…"
          : state === "running"
            ? "Waiting for browser…"
            : complete
              ? (completeActionLabel ?? "Signed in")
              : resumable
                ? "Continue sign-in"
                : `Sign in to ${title}`}
      </button>
      {state === "running" && onCancel ? (
        <button
          type="button"
          className="mt-3 text-xs text-content/65 underline"
          onClick={onCancel}
        >
          {cancelLabel ?? "Cancel sign-in"}
        </button>
      ) : null}
      {state === "running" && loginUrl ? (
        <div className="mt-3 flex max-w-60 flex-col items-center gap-2 text-center text-xs text-content/65">
          <p>
            To use another browser, copy the sign-in link and open it there.
          </p>
          <button type="button" className="underline" onClick={onCopyUrl}>
            Copy sign-in link
          </button>
          {onSubmitCode ? (
            <form
              onSubmit={(event) => {
                event.preventDefault();
                const input = event.currentTarget.elements.namedItem(
                  "code",
                ) as HTMLInputElement;
                if (input.value.trim()) {
                  onSubmitCode(input.value);
                  input.value = "";
                }
              }}
            >
              <label htmlFor="provider-sign-in-code">
                If the browser shows a code, paste it here
              </label>
              <input
                id="provider-sign-in-code"
                name="code"
                className="mt-1 w-full rounded border border-content/20 bg-transparent px-2 py-1"
                autoComplete="off"
              />
              <button type="submit" className="mt-2 underline">
                Submit code
              </button>
            </form>
          ) : null}
        </div>
      ) : null}
      {error ? (
        <p
          className="mt-2.5 max-w-60 text-[10px] leading-4 text-red-500"
          role="status"
        >
          {error}
        </p>
      ) : null}
    </div>
  );
}
