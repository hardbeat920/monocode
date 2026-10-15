import { useEffect, useRef, useState } from "react";
import { Pencil, X } from "../../../shared/ui/icons";
import { FileTypeIcon } from "../../files/ui/FileTypeIcon";
import { Popover } from "../../../shared/ui/Popover";
import {
  chatReferencesLabel,
  removeChatReference,
  updateChatReferenceNote,
  type ChatReference,
} from "../model/chatReferences";

type Props = {
  references: ChatReference[];
  onChange: (next: ChatReference[]) => void;
};

const CLOSE_DELAY_MS = 150;

/** One pill for everything cited from the transcript; hover it to see or edit each. */
export function ChatReferencesPill({ references, onChange }: Props) {
  const pill = useRef<HTMLDivElement>(null);
  const closeTimer = useRef<number | undefined>(undefined);
  const [open, setOpen] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editValue, setEditValue] = useState("");

  useEffect(() => () => window.clearTimeout(closeTimer.current), []);
  // Removing the last one hides the pill; the next one must not reopen the list.
  const empty = references.length === 0;
  useEffect(() => {
    if (!empty) return;
    window.clearTimeout(closeTimer.current);
    setOpen(false);
    setEditingId(null);
  }, [empty]);

  if (references.length === 0) return null;

  const show = () => {
    window.clearTimeout(closeTimer.current);
    setOpen(true);
  };
  const hideSoon = () => {
    window.clearTimeout(closeTimer.current);
    closeTimer.current = window.setTimeout(() => {
      setOpen(false);
      setEditingId(null);
    }, CLOSE_DELAY_MS);
  };
  const saveEdit = () => {
    if (editingId) {
      onChange(updateChatReferenceNote(references, editingId, editValue));
    }
    setEditingId(null);
  };

  return (
    <>
      <div
        ref={pill}
        data-chat-references-pill
        onMouseEnter={show}
        onMouseLeave={() => {
          if (!editingId) hideSoon();
        }}
        className="group relative flex min-w-0 items-center gap-1.5 rounded-md bg-content/10 py-0.5 pl-1 pr-1"
      >
        <button
          type="button"
          aria-expanded={open}
          onClick={() => (open ? setOpen(false) : show())}
          className="flex min-w-0 items-center gap-1.5 rounded focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
        >
          <span className="grid size-5 shrink-0 place-items-center">
            <FileTypeIcon name="annotations.txt" isDir={false} size={16} />
          </span>
          <span className="min-w-0 max-w-[140px] truncate text-[11px] leading-none text-content/80">
            {chatReferencesLabel(references.length)}
          </span>
        </button>
        <button
          type="button"
          title="Remove all"
          aria-label="Remove all annotations"
          onClick={() => {
            setOpen(false);
            onChange([]);
          }}
          className="grid size-4 shrink-0 place-items-center rounded-full text-content/40 hover:bg-content/15 hover:text-content"
        >
          <X className="size-3" strokeWidth={2} />
        </button>
      </div>
      {open ? (
        <Popover
          anchor={pill}
          side="top"
          align="start"
          width={320}
          role="dialog"
          aria-label="Annotations"
          className="p-1"
          dismissOnEscape={!editingId}
          ignore="[data-chat-references-pill]"
          onDismiss={() => {
            setOpen(false);
            setEditingId(null);
          }}
          onMouseEnter={show}
          onMouseLeave={() => {
            if (!editingId) hideSoon();
          }}
        >
          <ul className="flex max-h-72 flex-col gap-0.5 overflow-y-auto">
            {references.map((reference, index) => (
              <li
                key={reference.id}
                className="group rounded-lg px-2.5 py-2 hover:bg-content/5"
              >
                <div className="flex items-start gap-2">
                  <span className="mt-0.5 shrink-0 text-[11px] text-content/40">
                    {index + 1}
                  </span>
                  <div className="min-w-0 flex-1 border-l-2 border-content/15 pl-2">
                    {reference.source ? (
                      <p className="truncate text-[11px] leading-snug text-content/40">
                        {reference.source.label}
                      </p>
                    ) : null}
                    <p className="line-clamp-3 whitespace-pre-wrap text-xs leading-snug text-content/60">
                      {reference.text}
                    </p>
                  </div>
                  <button
                    type="button"
                    title="Remove"
                    aria-label={`Remove annotation ${index + 1}`}
                    onClick={() => {
                      if (editingId === reference.id) setEditingId(null);
                      onChange(removeChatReference(references, reference.id));
                    }}
                    className="grid size-4 shrink-0 place-items-center rounded-full text-content/40 hover:bg-content/15 hover:text-content"
                  >
                    <X className="size-3" strokeWidth={2} />
                  </button>
                </div>
                {editingId === reference.id ? (
                  <textarea
                    autoFocus
                    aria-label={`Note for annotation ${index + 1}`}
                    value={editValue}
                    rows={2}
                    onChange={(event) => setEditValue(event.target.value)}
                    onBlur={saveEdit}
                    onKeyDown={(event) => {
                      if (event.key === "Escape") {
                        event.preventDefault();
                        event.stopPropagation();
                        setEditingId(null);
                      } else if (
                        event.key === "Enter" &&
                        !event.shiftKey &&
                        !event.nativeEvent.isComposing
                      ) {
                        event.preventDefault();
                        saveEdit();
                      }
                    }}
                    className="mt-1.5 w-full resize-none rounded-md bg-content/5 px-2 py-1.5 font-sans text-xs text-content outline-none ring-accent/40 focus:ring-2"
                  />
                ) : (
                  <button
                    type="button"
                    onClick={() => {
                      setEditingId(reference.id);
                      setEditValue(reference.note);
                    }}
                    className="mt-1.5 flex w-full items-start gap-1.5 rounded-md text-left text-xs leading-snug text-content/90 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                  >
                    <span
                      className={`min-w-0 flex-1 whitespace-pre-wrap ${
                        reference.note ? "" : "text-content/40"
                      }`}
                    >
                      {reference.note || "Add a note"}
                    </span>
                    <Pencil
                      aria-hidden="true"
                      className="mt-0.5 size-3 shrink-0 text-content/40 opacity-0 group-hover:opacity-100"
                      strokeWidth={1.75}
                    />
                  </button>
                )}
              </li>
            ))}
          </ul>
        </Popover>
      ) : null}
    </>
  );
}
