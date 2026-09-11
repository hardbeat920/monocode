import { FilePlusCorner, MessageSquarePlus } from "../chrome/icons";
import { useEffect, useRef, type ReactNode } from "react";
import { Popover } from "../chrome/Popover";
import { type TranscriptSelection } from "../lib/transcriptSelection";

type Props = {
  selection: TranscriptSelection | null;
  onAddToChat?: (text: string) => void;
  onAddToNotes?: (text: string) => void;
  onDismiss: () => void;
};

export function TranscriptSelectionMenu({
  selection,
  onAddToChat,
  onAddToNotes,
  onDismiss,
}: Props) {
  const onDismissRef = useRef(onDismiss);
  onDismissRef.current = onDismiss;

  // Scrolling or resizing moves the text out from under the menu, so the
  // selection it acts on is gone; drop it rather than chase the range.
  useEffect(() => {
    if (!selection) return;
    const dismiss = () => onDismissRef.current();
    window.addEventListener("scroll", dismiss, true);
    window.addEventListener("resize", dismiss);
    return () => {
      window.removeEventListener("scroll", dismiss, true);
      window.removeEventListener("resize", dismiss);
    };
  }, [selection]);

  if (!selection) return null;

  return (
    <Popover
      anchor={selection.rect}
      side="top"
      align="center"
      onDismiss={(reason) => {
        if (reason === "escape") window.getSelection()?.removeAllRanges();
        onDismiss();
      }}
      role="toolbar"
      aria-label="Selected text actions"
      className="p-1"
    >
      <div className="flex min-w-36 flex-col items-stretch gap-0.5">
        {onAddToChat ? (
          <SelectionAction
            label="Add to chat"
            onSelect={() => onAddToChat(selection.text)}
            onDismiss={onDismiss}
          >
            <MessageSquarePlus
              aria-hidden="true"
              className="size-3.5"
              strokeWidth={1.75}
            />
          </SelectionAction>
        ) : null}
        {onAddToNotes ? (
          <SelectionAction
            label="Add to notes"
            onSelect={() => onAddToNotes(selection.text)}
            onDismiss={onDismiss}
          >
            <FilePlusCorner
              aria-hidden="true"
              className="size-3.5"
              strokeWidth={1.75}
            />
          </SelectionAction>
        ) : null}
      </div>
    </Popover>
  );
}

function SelectionAction({
  label,
  children,
  onSelect,
  onDismiss,
}: {
  label: string;
  children: ReactNode;
  onSelect: () => void;
  onDismiss: () => void;
}) {
  return (
    <button
      type="button"
      onMouseDown={(event) => event.preventDefault()}
      onClick={() => {
        onSelect();
        window.getSelection()?.removeAllRanges();
        onDismiss();
      }}
      className="flex h-8 w-full items-center gap-2 whitespace-nowrap rounded-lg px-2.5 font-sans text-[13px] leading-none text-content outline-none ring-accent/40 hover:bg-content/5 focus-visible:ring-2"
    >
      {children}
      {label}
    </button>
  );
}
