import type { HarnessId } from "../model/session";
import { HARNESS_TITLE } from "../model/session";
import { HarnessIcon } from "./HarnessIcon";
import { Check, RefreshCw } from "../../../shared/ui/icons";
import { useTranslation } from "../../i18n/model/i18n";

export type ProviderSignInState = "idle" | "running" | "complete" | "error";

export function ProviderSignInPanel({
  harness,
  state,
  error,
  onSignIn,
  onComplete,
  completeActionLabel,
  autoFocus = false,
}: {
  harness: HarnessId;
  state: ProviderSignInState;
  error: string | null;
  onSignIn: () => void;
  onComplete?: () => void;
  completeActionLabel?: string;
  autoFocus?: boolean;
}) {
  const { t } = useTranslation();
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
        {complete
          ? t("dialogs.providerSignInSuccess", `Signed in to ${title}`, { provider: title })
          : t("dialogs.providerSignInTitle", "Authentication required")}
      </h2>
      <p className="mt-1 max-w-56 text-[11px] leading-4 text-content/45 text-center">
        {complete
          ? t("dialogs.providerSignInSuccessHelp", "You can retry your last message now.")
          : t("dialogs.providerSignInDescription", `Sign in to continue using ${title}.`, {
              provider: title,
            })}
      </p>
      <button
        type="button"
        autoFocus={autoFocus}
        className="mt-4 inline-flex h-8 items-center gap-1.5 rounded-lg bg-content px-3.5 text-[12px] font-medium text-background-base transition-transform duration-150 ease-out hover:bg-content/85 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent active:scale-[0.98] disabled:cursor-default disabled:opacity-55 text-center"
        disabled={state === "running" || (complete && !onComplete)}
        onClick={complete && onComplete ? onComplete : onSignIn}
      >
        {state === "running" ? (
          <RefreshCw className="size-3.5 animate-spin" aria-hidden />
        ) : complete ? (
          <Check className="size-3.5" aria-hidden />
        ) : null}
        {state === "running"
          ? t("dialogs.providerSignInWaiting", "Waiting for browser…")
          : complete
            ? (completeActionLabel ?? t("dialogs.providerSignInComplete", "Signed in"))
            : t("dialogs.providerSignInAction", `Sign in to ${title}`, { provider: title })}
      </button>
      {state === "error" && error ? (
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
