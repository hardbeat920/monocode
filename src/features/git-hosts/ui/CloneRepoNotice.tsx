import { useEffect } from "react";
import { LAYER } from "../../../shared/lib/layers";

const AUTO_DISMISS_MS = 4_000;

/** Confirms what the clone dialog just did, since opening an existing
 * checkout and cloning a new one otherwise look identical to the user. */
export function CloneRepoNotice({
  message,
  onDismiss,
}: {
  message: string;
  onDismiss: () => void;
}) {
  useEffect(() => {
    const timer = setTimeout(onDismiss, AUTO_DISMISS_MS);
    return () => clearTimeout(timer);
  }, [message, onDismiss]);

  return (
    <div
      role="status"
      className="fixed bottom-4 right-4 flex max-w-sm items-start gap-3 rounded-xl border border-content/15 bg-[#252525] px-3 py-2 text-xs text-content/80 shadow-xl"
      style={{ zIndex: LAYER.toast }}
    >
      <span className="min-w-0 break-words">{message}</span>
      <button
        type="button"
        aria-label="Dismiss"
        className="shrink-0 text-content/50 hover:text-content/80"
        onClick={onDismiss}
      >
        Dismiss
      </button>
    </div>
  );
}
