import { Modal } from "../../../shared/ui/Modal";

/** Asks before changing a connected machine's branch under running sessions. */
export function SwitchWhileRunningDialog({
  branch,
  creating = false,
  message,
  busy,
  error,
  onConfirm,
  onCancel,
}: {
  branch: string;
  creating?: boolean;
  /** The host's explanation, which names the running sessions. */
  message: string;
  busy: boolean;
  error?: string | null;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  return (
    <Modal
      title={creating ? `Create ${branch} anyway?` : `Switch to ${branch} anyway?`}
      size="sm"
      onClose={() => {
        if (!busy) onCancel();
      }}
    >
      <div className="flex flex-col gap-4 p-4 text-[12px]">
        <p>{message.replace(/^Host rejected request:\s*/, "")}</p>
        {error ? (
          <p role="alert" className="whitespace-pre-wrap text-red-400/90">
            {error}
          </p>
        ) : null}
        <div className="flex justify-end gap-2">
          <button
            type="button"
            disabled={busy}
            onClick={onCancel}
            className="rounded-md px-3 py-1.5 hover:bg-content/8 active:scale-[0.97] disabled:opacity-50"
          >
            Cancel
          </button>
          <button
            type="button"
            disabled={busy}
            onClick={onConfirm}
            className="rounded-md bg-red-500/20 px-3 py-1.5 font-medium text-red-400 hover:bg-red-500/30 active:scale-[0.97] disabled:opacity-50"
          >
            {creating ? "Create and switch" : "Switch anyway"}
          </button>
        </div>
      </div>
    </Modal>
  );
}
