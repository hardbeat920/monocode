import { useRef } from "react";
import { AlertCircle, Trash2, X } from "../../../shared/ui/icons";
import { GlassBackdrop } from "../../../app/shell/GlassBackdrop";
import { LAYER } from "../../../shared/lib/layers";
import type { ProjectBoardCard } from "./types";
import { useDialogFocusTrap } from "./useDialogFocusTrap";

export interface DeleteConfirmModalProps {
  card: ProjectBoardCard;
  onConfirm: () => void | Promise<void>;
  onCancel: () => void;
  isDeleting?: boolean;
}

export function DeleteConfirmModal({
  card,
  onConfirm,
  onCancel,
  isDeleting = false,
}: DeleteConfirmModalProps) {
  const dialogRef = useRef<HTMLDivElement>(null);
  const cancelButtonRef = useRef<HTMLButtonElement>(null);

  useDialogFocusTrap(dialogRef, {
    onClose: onCancel,
    initialFocusRef: cancelButtonRef,
  });

  const mediaCount = card.media?.length ?? 0;

  return (
    <div
      ref={dialogRef}
      role="dialog"
      aria-modal="true"
      aria-labelledby="delete-dialog-title"
      aria-describedby="delete-dialog-description"
      className="fixed inset-0 flex items-center justify-center p-4"
      style={{ zIndex: LAYER.dialogPopover }}
    >
      <div
        className="absolute inset-0 bg-black/60 backdrop-blur-sm"
        onClick={onCancel}
      />
      <div className="relative isolate z-10 flex w-full max-w-md flex-col overflow-hidden rounded-xl border border-content/10 bg-background-base shadow-2xl">
        <GlassBackdrop className="bg-background-base/90" />

        <header className="relative z-10 flex items-center justify-between border-b border-content/8 px-4 py-3">
          <div className="flex items-center gap-2 text-rose-400">
            <AlertCircle className="size-4 shrink-0" />
            <h3 id="delete-dialog-title" className="text-sm font-semibold text-content">
              Delete card
            </h3>
          </div>
          <button
            type="button"
            aria-label="Close confirmation"
            onClick={onCancel}
            className="grid size-7 place-items-center rounded-md text-content/50 hover:bg-content/8 hover:text-content focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
          >
            <X className="size-4" strokeWidth={1.75} />
          </button>
        </header>

        <div className="relative z-10 p-5">
          <p id="delete-dialog-description" className="mb-3 text-xs leading-relaxed text-content/80">
            Are you sure you want to delete <span className="font-semibold text-content wrap-anywhere">"{card.title}"</span>?
          </p>
          <div className="rounded-lg border border-rose-500/20 bg-rose-500/10 p-3 text-xs text-rose-300">
            <p className="font-medium">Attachment cascade warning:</p>
            <p className="mt-1 text-[11px] leading-relaxed text-rose-300/80">
              Deleting this card permanently removes all card data and its {mediaCount} associated image attachment{mediaCount === 1 ? "" : "s"}. This action cannot be undone.
            </p>
          </div>
        </div>

        <footer className="relative z-10 flex items-center justify-end gap-2 border-t border-content/8 px-4 py-3">
          <button
            ref={cancelButtonRef}
            type="button"
            onClick={onCancel}
            disabled={isDeleting}
            className="rounded-lg border border-content/10 bg-content/4 px-3 py-1.5 text-xs font-medium text-content/80 hover:bg-content/8 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
          >
            Cancel
          </button>
          <button
            type="button"
            onClick={onConfirm}
            disabled={isDeleting}
            className="flex items-center gap-1.5 rounded-lg bg-red-500 px-3 py-1.5 text-xs font-medium text-white shadow-sm hover:bg-red-600 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-red-400 disabled:opacity-50"
          >
            <Trash2 className="size-3.5" />
            <span>{isDeleting ? "Deleting..." : "Delete card"}</span>
          </button>
        </footer>
      </div>
    </div>
  );
}
