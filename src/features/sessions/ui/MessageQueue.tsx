import { useEffect, useRef, useState } from "react";
import {
  Check,
  CornerDownRight,
  ListEnd,
  Pause,
  Pencil,
  Play,
  Trash2,
  X,
} from "../../../shared/ui/icons";
import { isImeComposition } from "../../../shared/lib/keyboard";
import { useT } from "../../../shared/hooks/useI18n";
import type { MessageQueueStatus, QueuedMessage } from "../model/session";

export function MessageQueue({
  messages,
  status,
  onDelete,
  onEdit,
  onEditingChange,
  onSteer,
  onResume,
  variant = "queue",
  sendingId,
}: {
  messages: QueuedMessage[];
  status?: MessageQueueStatus;
  onDelete?: (messageId: string) => void;
  onEdit?: (messageId: string, text: string) => void;
  onEditingChange?: (messageId?: string) => void;
  onSteer?: (messageId: string) => void;
  onResume?: () => void;
  variant?: "queue" | "messages";
  sendingId?: string;
}) {
  const t = useT();
  const [editingId, setEditingId] = useState<string>();
  const [editDraft, setEditDraft] = useState("");
  const onEditingChangeRef = useRef(onEditingChange);
  onEditingChangeRef.current = onEditingChange;
  const editingIdRef = useRef(editingId);
  editingIdRef.current = editingId;
  useEffect(() => {
    return () => {
      if (editingIdRef.current) onEditingChangeRef.current?.();
    };
  }, []);
  if (messages.length === 0) return null;
  const paused = status === "paused";
  const failed = messages.some((message) => message.error);

  const startEdit = (message: QueuedMessage) => {
    setEditingId(message.id);
    setEditDraft(message.text);
    onEditingChange?.(message.id);
  };
  const cancelEdit = () => {
    setEditingId(undefined);
    setEditDraft("");
    onEditingChange?.();
  };
  const saveEdit = (message: QueuedMessage) => {
    if (!editDraft.trim() && message.attachments.length === 0) return;
    onEdit?.(message.id, editDraft);
    setEditingId(undefined);
    setEditDraft("");
  };

  return (
    <div className="px-2 text-content/55" data-message-queue>
      <div
        className="relative z-0 rounded-t-[10px] border border-b-0 border-content/10 bg-content/3 px-2 py-1"
        data-message-queue-card
      >
        {paused ? (
          <div className="flex h-7 items-center gap-2 border-b border-stroke text-[12px]">
            <Pause className="size-3.5" />
            <span className="min-w-0 flex-1 truncate">
              {failed
                ? "A message couldn't be sent"
                : variant === "messages"
                  ? "Messages paused"
                  : t("Queue paused because you interrupted")}
            </span>
            <button
              type="button"
              onClick={onResume}
              className="flex h-6 shrink-0 items-center gap-1.5 rounded-md px-1.5 hover:bg-content/10 hover:text-content"
            >
              <Play className="size-3.5" />
              {failed ? "Retry" : t("Resume")}
            </button>
          </div>
        ) : null}
        {variant === "messages" && !paused ? (
          <div role="status" className="py-1 text-[11px] text-content/50">
            {sendingId ? "Sending…" : "Waiting to send"}
          </div>
        ) : null}
        {messages.map((message, index) => {
          const sending = sendingId === message.id;
          const editing = editingId === message.id;
          const label =
            (message.monoSessionCompletion
              ? message.monoSessionCompletion.sessionCount
                ? `${message.monoSessionCompletion.sessionCount} sessions finished`
                : `Session ${message.monoSessionCompletion.status}: ${message.monoSessionCompletion.title}`
              : message.text.trim()) ||
            message.noteCard?.title ||
            message.handoffCard?.brief ||
            (message.attachments.length === 1
              ? t("1 attachment")
              : t("{count} attachments", {
                  count: message.attachments.length,
                }));
          return (
            <div
              key={message.id}
              className={`flex min-h-7 items-center gap-2 text-[12px] ${
                index > 0 ? "border-t border-stroke" : ""
              }`}
            >
              <ListEnd className="size-3.5 shrink-0" />
              {editing ? (
                <>
                  <textarea
                    autoFocus
                    aria-label={t("Edit queued message")}
                    value={editDraft}
                    rows={1}
                    onChange={(event) => setEditDraft(event.target.value)}
                    onKeyDown={(event) => {
                      if (isImeComposition(event.nativeEvent)) return;
                      if (event.key === "Escape") {
                        event.preventDefault();
                        cancelEdit();
                      } else if (event.key === "Enter" && !event.shiftKey) {
                        event.preventDefault();
                        saveEdit(message);
                      }
                    }}
                    className="min-h-6 min-w-0 flex-1 resize-none rounded-md border border-content/15 bg-content/5 px-1.5 py-0.5 text-[12px] text-content outline-none focus:border-content/30"
                  />
                  <button
                    type="button"
                    title={t("Save queued message")}
                    aria-label={t("Save queued message")}
                    disabled={
                      !editDraft.trim() && message.attachments.length === 0
                    }
                    onClick={() => saveEdit(message)}
                    className="grid size-6 shrink-0 place-items-center rounded-md hover:bg-content/10 hover:text-content disabled:opacity-30"
                  >
                    <Check className="size-3.5" />
                  </button>
                  <button
                    type="button"
                    title={t("Cancel queued message edit")}
                    aria-label={t("Cancel queued message edit")}
                    onClick={cancelEdit}
                    className="grid size-6 shrink-0 place-items-center rounded-md hover:bg-content/10 hover:text-content"
                  >
                    <X className="size-3.5" />
                  </button>
                </>
              ) : (
                <>
                  <div className="min-w-0 flex-1 py-1">
                    <div className="truncate text-content/80" title={label}>
                      {label}
                    </div>
                    {message.error ? (
                      <p role="alert" className="text-red-400">
                        {message.error}
                      </p>
                    ) : null}
                  </div>
                  {onSteer && !message.monoSessionCompletion ? (
                    <button
                      type="button"
                      disabled={sending}
                      onClick={() => onSteer?.(message.id)}
                      className="flex h-6 shrink-0 items-center gap-1.5 rounded-md px-1.5 hover:bg-content/10 hover:text-content"
                    >
                      <CornerDownRight className="size-3.5" />
                      {t("Steer")}
                    </button>
                  ) : null}
                  <button
                    type="button"
                    title={t("Edit queued message")}
                    aria-label={t("Edit queued message")}
                    disabled={sending || !!message.monoSessionCompletion}
                    onClick={() => startEdit(message)}
                    className="grid size-6 shrink-0 place-items-center rounded-md hover:bg-content/10 hover:text-content"
                  >
                    <Pencil className="size-3.5" />
                  </button>
                  <button
                    type="button"
                    title={t("Remove queued message")}
                    aria-label={t("Remove queued message")}
                    disabled={sending}
                    onClick={() => onDelete?.(message.id)}
                    className="grid size-6 shrink-0 place-items-center rounded-md hover:bg-content/10 hover:text-content"
                  >
                    <Trash2 className="size-3.5" />
                  </button>
                </>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}
