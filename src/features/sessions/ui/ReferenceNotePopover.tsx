import { useEffect, useRef, useState } from "react";
import { Popover } from "../../../shared/ui/Popover";

type Props = {
  /** Where the cited text is on screen. */
  anchor: DOMRect;
  text: string;
  initialNote?: string;
  submitLabel: string;
  onSubmit: (note: string) => void;
  onCancel: () => void;
  /** Offered when editing a reference that is already added. */
  onRemove?: () => void;
};

/** A note box over the cited text, used to add a reference and to edit one later. */
export function ReferenceNotePopover({
  anchor,
  text,
  initialNote = "",
  submitLabel,
  onSubmit,
  onCancel,
  onRemove,
}: Props) {
  const [note, setNote] = useState(initialNote);
  const field = useRef<HTMLTextAreaElement>(null);
  // The cursor lands in the note immediately, after the popover has placed
  // itself, so the user can start typing without a click.
  useEffect(() => {
    const frame = requestAnimationFrame(() => {
      const el = field.current;
      if (!el) return;
      el.focus({ preventScroll: true });
      el.setSelectionRange(el.value.length, el.value.length);
    });
    return () => cancelAnimationFrame(frame);
  }, []);
  return (
    <Popover
      anchor={anchor}
      side="top"
      align="center"
      width={300}
      onDismiss={onCancel}
      role="dialog"
      aria-label="Add a note to the selection"
      className="p-2"
    >
      <p className="mb-1.5 line-clamp-2 border-l-2 border-content/15 pl-2 font-sans text-xs leading-snug text-content/60">
        {text}
      </p>
      <textarea
        ref={field}
        autoFocus
        rows={2}
        value={note}
        placeholder="Add a note (optional)"
        aria-label="Note"
        onChange={(event) => setNote(event.target.value)}
        onKeyDown={(event) => {
          if (
            event.key === "Enter" &&
            !event.shiftKey &&
            !event.nativeEvent.isComposing
          ) {
            event.preventDefault();
            onSubmit(note);
          }
        }}
        className="w-full resize-none rounded-md bg-content/5 px-2 py-1.5 font-sans text-[13px] text-content outline-none ring-accent/40 placeholder:text-content/40 focus:ring-2"
      />
      <div className="mt-1.5 flex items-center justify-between gap-2 font-sans text-[11px] text-content/40">
        <span className="truncate">
          Enter to save · Shift+Enter for a new line
        </span>
        <span className="flex shrink-0 items-center gap-1">
          {onRemove ? (
            <button
              type="button"
              onClick={onRemove}
              className="rounded-md px-2 py-1 text-xs text-content/70 hover:bg-content/10 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
            >
              Remove
            </button>
          ) : null}
          <button
            type="button"
            onClick={() => onSubmit(note)}
            className="rounded-md bg-content/10 px-2 py-1 text-xs text-content hover:bg-content/15 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
          >
            {submitLabel}
          </button>
        </span>
      </div>
    </Popover>
  );
}
