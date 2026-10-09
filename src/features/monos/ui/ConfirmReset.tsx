import { useRef, useState, type ReactNode, type RefObject } from "react";
import { Modal } from "../../../shared/ui/Modal";

/** A destructive reset that asks first, then reports a failure in place. */
export function ConfirmReset({
  label,
  title,
  body,
  kept,
  failure,
  onConfirm,
  children,
}: {
  /** The confirm button, and what the trigger says. */
  label: string;
  title: string;
  body: string;
  /** What survives, said separately so it reads as reassurance. */
  kept: string;
  failure: string;
  onConfirm: () => Promise<void>;
  /** The trigger; focus returns to `ref` when the dialog closes. */
  children: (
    open: () => void,
    ref: RefObject<HTMLButtonElement | null>,
  ) => ReactNode;
}) {
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  const running = useRef(false);
  const trigger = useRef<HTMLButtonElement>(null);
  const close = () => {
    if (running.current) return;
    setConfirming(false);
    trigger.current?.focus();
  };
  const reset = async () => {
    if (running.current) return;
    running.current = true;
    setBusy(true);
    setError(undefined);
    try {
      await onConfirm();
      setConfirming(false);
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : String(failure));
    } finally {
      running.current = false;
      setBusy(false);
    }
  };
  return (
    <>
      {children(() => {
        setError(undefined);
        setConfirming(true);
      }, trigger)}
      {confirming ? (
        <Modal title={title} size="sm" onClose={close}>
          <div className="flex flex-col gap-3 px-4 pb-4 pt-3">
            <p className="text-[13px] leading-5 text-content/70">{body}</p>
            <p className="text-[12px] leading-5 text-content/50">{kept}</p>
            {error ? (
              <p role="alert" className="text-[12px] leading-5 text-red-400">
                {failure} {error}
              </p>
            ) : null}
            <div className="flex justify-end gap-2 pt-1">
              <button
                type="button"
                disabled={busy}
                onClick={close}
                className="rounded-md px-3 py-1.5 text-[12px] text-content/70 enabled:hover:bg-content/8 disabled:opacity-50"
              >
                Cancel
              </button>
              <button
                type="button"
                disabled={busy}
                onClick={() => void reset()}
                className="rounded-md bg-red-500/20 px-3 py-1.5 text-[12px] font-medium text-red-300 enabled:hover:bg-red-500/30 disabled:opacity-50"
              >
                {busy ? "Resetting…" : label}
              </button>
            </div>
          </div>
        </Modal>
      ) : null}
    </>
  );
}
