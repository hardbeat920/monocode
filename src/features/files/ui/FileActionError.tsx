import { LAYER } from "../../../shared/lib/layers";

export function FileActionError({
  message,
  onDismiss,
}: {
  message: string;
  onDismiss: () => void;
}) {
  return (
    <div
      role="alert"
      className="fixed bottom-4 right-4 flex max-w-sm items-start gap-3 rounded-xl border border-red-400/30 bg-[#252525] px-3 py-2 text-xs text-red-300 shadow-xl"
      style={{ zIndex: LAYER.toast }}
    >
      <span className="min-w-0 break-words">{message}</span>
      <button
        type="button"
        aria-label="Dismiss file action error"
        className="shrink-0 text-content/60 hover:text-content"
        onClick={onDismiss}
      >
        Dismiss
      </button>
    </div>
  );
}
