import {
  useRef,
  useState,
  type ChangeEvent,
  type ClipboardEvent,
  type FormEvent,
} from "react";
import {
  ExternalLink,
  ImagePlus,
  LoaderCircle,
  Plus,
  Trash2,
  X,
} from "../../../shared/ui/icons";
import { GlassBackdrop } from "../../../app/shell/GlassBackdrop";
import { LAYER } from "../../../shared/lib/layers";
import {
  BOARD_LANES,
  BOARD_VALIDATION,
  type BoardMediaRef,
  type BoardPriority,
  type BoardStatus,
  type ProjectBoardCard,
  type ProjectBoardCardInput,
  type ProjectBoardCardPatch,
  type ProjectBoardRepository,
} from "./types";
import { MediaPreviewModal } from "./MediaPreviewModal";
import { useDialogFocusTrap } from "./useDialogFocusTrap";

interface Props {
  projectCwd: string;
  initialCard?: ProjectBoardCard | null;
  defaultStatus?: BoardStatus;
  repository: ProjectBoardRepository;
  onClose: () => void;
  onSaved: (card: ProjectBoardCard) => void;
}

interface PendingMedia {
  id: string;
  name: string;
  mimeType: BoardMediaRef["mimeType"];
  byteLength: number;
  dataBase64: string;
  isExisting?: boolean;
}

function generateId(): string {
  if (typeof crypto !== "undefined" && crypto.randomUUID) {
    return crypto.randomUUID();
  }
  return "card-" + Date.now().toString(36) + "-" + Math.random().toString(36).slice(2, 8);
}

export function CardEditorModal({
  projectCwd,
  initialCard,
  defaultStatus = "backlog",
  repository,
  onClose,
  onSaved,
}: Props) {
  const cardId = useRef<string>(initialCard?.id ?? generateId()).current;
  const isEditing = Boolean(initialCard);
  const createdRef = useRef(false);

  // Snapshot opening values for existing cards to submit only changed fields
  const openingSnapshotRef = useRef(
    initialCard
      ? {
          title: initialCard.title,
          description: initialCard.description,
          status: initialCard.status,
          priority: initialCard.priority,
        }
      : null,
  );

  const [title, setTitle] = useState(initialCard?.title ?? "");
  const [description, setDescription] = useState(initialCard?.description ?? "");
  const [status, setStatus] = useState<BoardStatus>(
    initialCard?.status ?? defaultStatus,
  );
  const [priority, setPriority] = useState<BoardPriority>(
    initialCard?.priority ?? "medium",
  );
  const existingSessions = useRef(initialCard?.linkedSessionIds ?? []).current;
  const [mediaList, setMediaList] = useState<PendingMedia[]>(() =>
    (initialCard?.media ?? []).map((m) => ({
      ...m,
      dataBase64: "",
      isExisting: true,
    })),
  );
  const [stagedRemovals, setStagedRemovals] = useState<string[]>([]);

  const [selectedPreview, setSelectedPreview] = useState<PendingMedia | null>(
    null,
  );
  const [error, setError] = useState<string | null>(null);
  const [isSubmitting, setIsSubmitting] = useState(false);

  const modalRef = useRef<HTMLDivElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const titleInputRef = useRef<HTMLInputElement>(null);

  useDialogFocusTrap(modalRef, {
    onClose,
    initialFocusRef: titleInputRef,
  });

  const handleProcessFile = (file: File) => {
    setError(null);
    const mime = file.type as BoardMediaRef["mimeType"];
    if (!BOARD_VALIDATION.mediaTypes.includes(mime)) {
      setError(`Unsupported image type "${file.type}". Allowed: PNG, JPEG, GIF, WebP.`);
      return;
    }
    if (file.size > BOARD_VALIDATION.mediaBytesMax) {
      setError(`Image "${file.name}" exceeds the 5 MiB limit (${Math.round(file.size / 1024)} KB).`);
      return;
    }

    const reader = new FileReader();
    reader.onload = () => {
      const result = reader.result as string;
      const base64Data = result.includes(",") ? result.split(",")[1] : result;
      const newMedia: PendingMedia = {
        id: "media-" + Date.now().toString(36) + "-" + Math.random().toString(36).slice(2, 6),
        name: file.name || "pasted-image.png",
        mimeType: mime,
        byteLength: file.size,
        dataBase64: base64Data,
        isExisting: false,
      };
      setMediaList((prev) => [...prev, newMedia]);
    };
    reader.onerror = () => {
      setError("Failed to read image file.");
    };
    reader.readAsDataURL(file);
  };

  const handleFileInputChange = (e: ChangeEvent<HTMLInputElement>) => {
    const files = e.target.files;
    if (!files || files.length === 0) return;
    for (let i = 0; i < files.length; i++) {
      handleProcessFile(files[i]);
    }
    e.target.value = "";
  };

  const handlePaste = (e: ClipboardEvent<HTMLDivElement>) => {
    const items = e.clipboardData?.items;
    if (!items) return;
    for (let i = 0; i < items.length; i++) {
      const item = items[i];
      const file = item.getAsFile();
      if (file) {
        e.preventDefault();
        handleProcessFile(file);
      }
    }
  };

  const handleRemoveMedia = (media: PendingMedia) => {
    if (media.isExisting) {
      setStagedRemovals((prev) =>
        prev.includes(media.id) ? prev : [...prev, media.id],
      );
    }
    setMediaList((prev) => prev.filter((m) => m.id !== media.id));
  };

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault();
    setError(null);

    const cleanTitle = title.trim();
    if (!cleanTitle) {
      setError("Card title is required.");
      return;
    }
    if (cleanTitle.length > BOARD_VALIDATION.titleMax) {
      setError(`Title must be at most ${BOARD_VALIDATION.titleMax} characters.`);
      return;
    }
    if (description.length > BOARD_VALIDATION.descriptionMax) {
      setError(`Description must be at most ${BOARD_VALIDATION.descriptionMax} characters.`);
      return;
    }

    setIsSubmitting(true);
    try {
      let savedCard: ProjectBoardCard;

      if (isEditing || createdRef.current) {
        const snapshot = openingSnapshotRef.current;
        if (!snapshot) {
          throw new Error("Card opening snapshot is unavailable.");
        }

        const patch: ProjectBoardCardPatch = {};
        if (cleanTitle !== snapshot.title) {
          patch.title = cleanTitle;
        }
        if (description !== snapshot.description) {
          patch.description = description;
        }
        if (status !== snapshot.status) {
          patch.status = status;
        }
        if (priority !== snapshot.priority) {
          patch.priority = priority;
        }

        savedCard = await repository.patchCard(projectCwd, cardId, patch);
      } else {
        const input: ProjectBoardCardInput = {
          id: cardId,
          projectCwd,
          title: cleanTitle,
          description,
          status,
          priority,
          linkedSessionIds: existingSessions,
        };
        savedCard = await repository.upsertCard(input);
        createdRef.current = true;
        openingSnapshotRef.current = {
          title: cleanTitle,
          description,
          status,
          priority,
        };
      }

      // Keep an evolving list so later upload updates preserve earlier persisted references.
      let mediaToUpload = [...mediaList];
      for (let i = 0; i < mediaToUpload.length; i++) {
        const media = mediaToUpload[i];
        if (!media.isExisting && media.dataBase64) {
          const uploadedRef = await repository.addMedia({
            projectCwd,
            cardId,
            name: media.name,
            mimeType: media.mimeType,
            dataBase64: media.dataBase64,
          });
          const persistedMedia: PendingMedia = {
            ...media,
            ...uploadedRef,
            dataBase64: "",
            isExisting: true,
          };
          const updatedMediaList = [...mediaToUpload];
          updatedMediaList[i] = persistedMedia;
          mediaToUpload = updatedMediaList;
          setMediaList(updatedMediaList);
        }
      }

      // Commit staged media removals only after card save succeeds
      let remainingRemovals = [...stagedRemovals];
      for (const mediaId of [...remainingRemovals]) {
        await repository.deleteMedia(projectCwd, cardId, mediaId);
        remainingRemovals = remainingRemovals.filter((id) => id !== mediaId);
        setStagedRemovals(remainingRemovals);
      }

      // Re-fetch card to get updated media list
      const updatedList = await repository.list(projectCwd);
      const refreshed = updatedList.find((c) => c.id === cardId) ?? savedCard;

      onSaved(refreshed);
      onClose();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to save card.");
    } finally {
      setIsSubmitting(false);
    }
  };

  return (
    <div
      ref={modalRef}
      role="dialog"
      aria-modal="true"
      aria-label={isEditing ? "Edit card" : "New card"}
      onPaste={handlePaste}
      className="fixed inset-0 flex items-center justify-center p-3 sm:p-6"
      style={{ zIndex: LAYER.dialog }}
    >
      <div
        className="absolute inset-0 bg-black/55 backdrop-blur-sm"
        onClick={onClose}
      />

      <div className="relative isolate z-10 flex max-h-[calc(100dvh-24px)] w-full max-w-xl flex-col overflow-hidden rounded-2xl border border-content/10 bg-background-base shadow-2xl">
        <GlassBackdrop className="bg-background-base/90" />

        <header className="relative z-10 flex shrink-0 items-center justify-between border-b border-content/8 px-5 py-3.5">
          <div className="min-w-0 flex-1">
            <h2 className="text-base font-semibold text-content">
              {isEditing ? "Edit card" : "New card"}
            </h2>
            <p className="text-xs text-content/50">
              {projectCwd}
            </p>
          </div>
          <button
            type="button"
            aria-label="Close"
            onClick={onClose}
            className="grid size-7 place-items-center rounded-md text-content/50 hover:bg-content/8 hover:text-content focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
          >
            <X className="size-4" strokeWidth={1.75} />
          </button>
        </header>

        <form
          onSubmit={handleSubmit}
          className="relative z-10 flex min-h-0 flex-1 flex-col overflow-y-auto p-5"
        >
          {error ? (
            <div
              role="alert"
              className="mb-4 rounded-lg border border-red-500/20 bg-red-500/10 p-3 text-xs text-red-400"
            >
              {error}
            </div>
          ) : null}

          {/* Title */}
          <div className="mb-4">
            <div className="mb-1.5 flex items-center justify-between">
              <label
                htmlFor="card-title-input"
                className="text-xs font-medium text-content/80"
              >
                Title <span className="text-red-400">*</span>
              </label>
              <span className="text-[11px] text-content/40">
                {title.length}/{BOARD_VALIDATION.titleMax}
              </span>
            </div>
            <input
              id="card-title-input"
              ref={titleInputRef}
              type="text"
              required
              maxLength={BOARD_VALIDATION.titleMax}
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              placeholder="What needs to be done?"
              className="w-full rounded-lg border border-content/10 bg-content/4 px-3 py-2 text-sm text-content placeholder:text-content/30 focus:border-accent focus:bg-background-base focus:outline-none focus:ring-1 focus:ring-accent"
            />
          </div>

          {/* Lane & Priority Row */}
          <div className="mb-4 grid grid-cols-1 gap-3 sm:grid-cols-2">
            <div>
              <label
                htmlFor="card-status-select"
                className="mb-1.5 block text-xs font-medium text-content/80"
              >
                Status lane
              </label>
              <select
                id="card-status-select"
                value={status}
                onChange={(e) => setStatus(e.target.value as BoardStatus)}
                className="w-full rounded-lg border border-content/10 bg-content/4 px-3 py-2 text-sm text-content focus:border-accent focus:bg-background-base focus:outline-none focus:ring-1 focus:ring-accent"
              >
                {BOARD_LANES.map((lane) => (
                  <option key={lane.status} value={lane.status}>
                    {lane.label}
                  </option>
                ))}
              </select>
            </div>

            <div>
              <label
                htmlFor="card-priority-select"
                className="mb-1.5 block text-xs font-medium text-content/80"
              >
                Priority
              </label>
              <select
                id="card-priority-select"
                value={priority}
                onChange={(e) => setPriority(e.target.value as BoardPriority)}
                className="w-full rounded-lg border border-content/10 bg-content/4 px-3 py-2 text-sm text-content focus:border-accent focus:bg-background-base focus:outline-none focus:ring-1 focus:ring-accent"
              >
                <option value="low">Low priority</option>
                <option value="medium">Medium priority</option>
                <option value="high">High priority</option>
              </select>
            </div>
          </div>

          {/* Description */}
          <div className="mb-4">
            <div className="mb-1.5 flex items-center justify-between">
              <label
                htmlFor="card-desc-textarea"
                className="text-xs font-medium text-content/80"
              >
                Description
              </label>
              <span className="text-[11px] text-content/40">
                {description.length}/{BOARD_VALIDATION.descriptionMax}
              </span>
            </div>
            <textarea
              id="card-desc-textarea"
              rows={4}
              maxLength={BOARD_VALIDATION.descriptionMax}
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              placeholder="Provide context, acceptance criteria, or steps..."
              className="w-full resize-y rounded-lg border border-content/10 bg-content/4 px-3 py-2 text-sm text-content placeholder:text-content/30 focus:border-accent focus:bg-background-base focus:outline-none focus:ring-1 focus:ring-accent"
            />
          </div>

          {/* Linked Sessions (Read-only) */}
          <div className="mb-4">
            <div className="mb-1.5 flex items-center justify-between">
              <span className="text-xs font-medium text-content/80">
                Linked sessions
              </span>
              {existingSessions.length > 0 ? (
                <span className="text-[11px] text-content/40">
                  {existingSessions.length} {existingSessions.length === 1 ? "thread" : "threads"}
                </span>
              ) : null}
            </div>
            {existingSessions.length > 0 ? (
              <div className="mb-1.5 flex flex-wrap gap-1.5">
                {existingSessions.map((sessionId) => (
                  <span
                    key={sessionId}
                    data-linked-session-id={sessionId}
                    className="inline-flex items-center gap-1 rounded border border-content/10 bg-content/4 px-2 py-1 text-xs font-mono text-content/80"
                  >
                    <ExternalLink className="size-2.5 opacity-60" />
                    <span className="max-w-[200px] truncate">{sessionId}</span>
                  </span>
                ))}
              </div>
            ) : null}
            <p className="text-[11px] text-content/40">
              Mono-started threads appear here.
            </p>
          </div>

          {/* Attachments / Media */}
          <div className="mb-5">
            <div className="mb-2 flex items-center justify-between">
              <span className="text-xs font-medium text-content/80">
                Image attachments ({mediaList.length})
              </span>
              <button
                type="button"
                onClick={() => fileInputRef.current?.click()}
                className="flex items-center gap-1 rounded-md px-2 py-1 text-xs font-medium text-accent hover:bg-accent/10 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
              >
                <Plus className="size-3.5" strokeWidth={1.75} />
                Add image
              </button>
            </div>

            <input
              ref={fileInputRef}
              type="file"
              multiple
              accept="image/png,image/jpeg,image/gif,image/webp"
              onChange={handleFileInputChange}
              className="hidden"
            />

            {mediaList.length > 0 ? (
              <div className="grid grid-cols-2 gap-2 sm:grid-cols-3">
                {mediaList.map((m) => {
                  const sizeKb = Math.round(m.byteLength / 1024);
                  return (
                    <div
                      key={m.id}
                      className="group relative flex flex-col overflow-hidden rounded-lg border border-content/10 bg-content/2 p-2 hover:border-content/20"
                    >
                      <button
                        type="button"
                        aria-label={`Preview ${m.name}`}
                        onClick={() => setSelectedPreview(m)}
                        className="flex flex-1 flex-col items-center justify-center gap-1 text-center"
                      >
                        {m.dataBase64 ? (
                          <img
                            src={`data:${m.mimeType};base64,${m.dataBase64}`}
                            alt={m.name}
                            className="h-16 w-full rounded object-cover"
                          />
                        ) : (
                          <div className="grid h-16 w-full place-items-center rounded bg-content/5 text-content/40">
                            <ImagePlus className="size-5" />
                          </div>
                        )}
                        <span className="max-w-full truncate text-[11px] font-medium text-content">
                          {m.name}
                        </span>
                        <span className="text-[10px] text-content/40">
                          {sizeKb} KB
                        </span>
                      </button>
                      <button
                        type="button"
                        aria-label={`Remove attachment ${m.name}`}
                        onClick={() => {
                          handleRemoveMedia(m);
                        }}
                        className="absolute right-1 top-1 grid size-5 place-items-center rounded bg-background-base/80 text-content/50 opacity-0 transition-opacity hover:text-red-400 group-hover:opacity-100 focus-visible:opacity-100"
                      >
                        <Trash2 className="size-3" />
                      </button>
                    </div>
                  );
                })}
              </div>
            ) : (
              <div
                onClick={() => fileInputRef.current?.click()}
                className="flex cursor-pointer flex-col items-center justify-center rounded-lg border border-dashed border-content/15 p-4 text-center transition-colors hover:border-content/30 hover:bg-content/2"
              >
                <ImagePlus className="mb-1.5 size-5 text-content/40" />
                <p className="text-xs text-content/60">
                  Click to browse or paste an image from your clipboard (Ctrl+V)
                </p>
                <p className="text-[10px] text-content/40">
                  PNG, JPEG, GIF, or WebP up to 5 MiB
                </p>
              </div>
            )}
          </div>

          {/* Form Actions */}
          <footer className="mt-auto flex items-center justify-end gap-2.5 border-t border-content/8 pt-4">
            <button
              type="button"
              onClick={onClose}
              disabled={isSubmitting}
              className="rounded-lg px-3.5 py-1.5 text-xs font-medium text-content/70 hover:bg-content/8 hover:text-content focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
            >
              Cancel
            </button>
            <button
              type="submit"
              disabled={isSubmitting}
              className="flex items-center gap-1.5 rounded-lg bg-accent px-4 py-1.5 text-xs font-medium text-white shadow-sm hover:brightness-110 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent disabled:opacity-50"
            >
              {isSubmitting ? (
                <>
                  <LoaderCircle className="size-3.5 animate-spin" />
                  Saving...
                </>
              ) : isEditing ? (
                "Save changes"
              ) : (
                "Create card"
              )}
            </button>
          </footer>
        </form>
      </div>

      {selectedPreview ? (
        <MediaPreviewModal
          projectCwd={projectCwd}
          cardId={cardId}
          mediaRef={selectedPreview}
          initialBase64={selectedPreview.dataBase64 || undefined}
          repository={repository}
          onClose={() => setSelectedPreview(null)}
          onDelete={() => {
            handleRemoveMedia(selectedPreview);
            setSelectedPreview(null);
          }}
        />
      ) : null}
    </div>
  );
}
