import { useEffect, useRef, useState } from "react";
import { LoaderCircle, Trash2, X } from "../../../shared/ui/icons";
import { GlassBackdrop } from "../../../app/shell/GlassBackdrop";
import { LAYER } from "../../../shared/lib/layers";
import type { BoardMediaRef, ProjectBoardRepository } from "./types";
import { useDialogFocusTrap } from "./useDialogFocusTrap";

interface Props {
  projectCwd: string;
  cardId: string;
  mediaRef: BoardMediaRef;
  initialBase64?: string;
  repository: ProjectBoardRepository;
  onClose: () => void;
  onDelete?: () => Promise<void> | void;
}

export function MediaPreviewModal({
  projectCwd,
  cardId,
  mediaRef,
  initialBase64,
  repository,
  onClose,
  onDelete,
}: Props) {
  const [base64, setBase64] = useState<string | null>(initialBase64 ?? null);
  const [loading, setLoading] = useState<boolean>(!initialBase64);
  const [error, setError] = useState<string | null>(null);
  const [isDeleting, setIsDeleting] = useState<boolean>(false);
  const [deleteError, setDeleteError] = useState<string | null>(null);

  const dialogRef = useRef<HTMLDivElement>(null);
  const closeButtonRef = useRef<HTMLButtonElement>(null);

  useDialogFocusTrap(dialogRef, {
    onClose,
    initialFocusRef: closeButtonRef,
  });

  useEffect(() => {
    if (initialBase64) return;
    let active = true;
    setLoading(true);
    setError(null);
    repository
      .getMedia(projectCwd, cardId, mediaRef.id)
      .then((res) => {
        if (!active) return;
        if (res?.dataBase64) {
          setBase64(res.dataBase64);
        } else {
          setError("Attachment could not be loaded");
        }
      })
      .catch((err) => {
        if (!active) return;
        setError(err instanceof Error ? err.message : "Failed to load attachment");
      })
      .finally(() => {
        if (active) setLoading(false);
      });

    return () => {
      active = false;
    };
  }, [cardId, initialBase64, mediaRef.id, projectCwd, repository]);

  const handleDelete = async () => {
    if (isDeleting) return;
    setIsDeleting(true);
    setDeleteError(null);
    try {
      if (onDelete) {
        await onDelete();
      } else {
        await repository.deleteMedia(projectCwd, cardId, mediaRef.id);
      }
      onClose();
    } catch (err) {
      const message = err instanceof Error ? err.message : "Unknown error";
      setDeleteError(`Failed to delete attachment: ${message}. Please retry or check file permissions.`);
    } finally {
      setIsDeleting(false);
    }
  };

  const imageSrc = base64
    ? base64.startsWith("data:")
      ? base64
      : `data:${mediaRef.mimeType};base64,${base64}`
    : "";

  const sizeKb = Math.round(mediaRef.byteLength / 1024);

  return (
    <div
      ref={dialogRef}
      role="dialog"
      aria-modal="true"
      aria-label={`Image preview: ${mediaRef.name}`}
      className="fixed inset-0 flex items-center justify-center p-4"
      style={{ zIndex: LAYER.dialogPopover }}
    >
      <div
        className="absolute inset-0 bg-black/60 backdrop-blur-sm"
        onClick={onClose}
      />
      <div className="relative isolate z-10 flex max-h-[calc(100dvh-48px)] w-full max-w-2xl flex-col overflow-hidden rounded-xl border border-content/10 bg-background-base shadow-2xl">
        <GlassBackdrop className="bg-background-base/80" />
        <header className="relative z-10 flex items-center justify-between border-b border-content/8 px-4 py-3">
          <div className="min-w-0 flex-1 pr-3">
            <h3 className="truncate text-sm font-semibold text-content">
              {mediaRef.name}
            </h3>
            <p className="text-[11px] text-content/50">
              {mediaRef.mimeType} · {sizeKb} KB
            </p>
          </div>
          <div className="flex items-center gap-1.5">
            {onDelete ? (
              <button
                type="button"
                aria-label={`Delete attachment ${mediaRef.name}`}
                title="Delete attachment"
                disabled={isDeleting}
                onClick={handleDelete}
                className="grid size-7 place-items-center rounded-md text-red-500 hover:bg-red-500/10 disabled:opacity-40 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-red-400"
              >
                {isDeleting ? (
                  <LoaderCircle className="size-3.5 animate-spin" />
                ) : (
                  <Trash2 className="size-3.5" strokeWidth={1.75} />
                )}
              </button>
            ) : null}
            <button
              ref={closeButtonRef}
              type="button"
              aria-label="Close preview"
              title="Close preview"
              onClick={onClose}
              className="grid size-7 place-items-center rounded-md text-content/60 hover:bg-content/8 hover:text-content focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
            >
              <X className="size-3.5" strokeWidth={1.75} />
            </button>
          </div>
        </header>

        {deleteError ? (
          <div
            role="alert"
            className="relative z-10 flex shrink-0 items-center justify-between border-b border-red-500/20 bg-red-500/10 px-4 py-2 text-xs text-red-400"
          >
            <span>{deleteError}</span>
            <button
              type="button"
              aria-label="Dismiss delete error"
              onClick={() => setDeleteError(null)}
              className="grid size-5 place-items-center rounded hover:bg-red-500/20"
            >
              <X className="size-3" />
            </button>
          </div>
        ) : null}

        <div className="relative z-10 flex min-h-[180px] max-h-[calc(100dvh-140px)] items-center justify-center overflow-auto p-4">
          {loading ? (
            <div className="flex flex-col items-center gap-2 text-content/50">
              <LoaderCircle className="size-6 animate-spin" />
              <span className="text-xs">Loading image...</span>
            </div>
          ) : error ? (
            <div className="text-center text-xs text-red-400">{error}</div>
          ) : imageSrc ? (
            <img
              src={imageSrc}
              alt={mediaRef.name}
              className="max-h-[calc(100dvh-160px)] max-w-full rounded-md object-contain"
            />
          ) : null}
        </div>
      </div>
    </div>
  );
}
