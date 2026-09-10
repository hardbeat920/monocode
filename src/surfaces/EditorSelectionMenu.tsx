import { useEffect, useRef } from "react";
import { MessageSquarePlus } from "../chrome/icons";
import { Popover } from "../chrome/Popover";
import {
  formatEditorSelectionReference,
  type EditorCodeSelection,
} from "../lib/editorSelection";
import { requestAddToChat } from "../lib/quoteDraft";

export type EditorSelectionTarget = EditorCodeSelection & {
  anchor: DOMRect;
};

export function EditorSelectionMenu({
  selection,
  onDismiss,
}: {
  selection: EditorSelectionTarget | null;
  onDismiss: () => void;
}) {
  const onDismissRef = useRef(onDismiss);
  onDismissRef.current = onDismiss;

  // A rect captured from CodeMirror is in viewport coordinates. Once the
  // editor moves, dismiss the action instead of leaving it over stale text.
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
      anchor={selection.anchor}
      side="top"
      align="center"
      gap={6}
      onDismiss={onDismiss}
      role="toolbar"
      aria-label="Selected code actions"
      className="p-1"
    >
      <button
        type="button"
        onMouseDown={(event) => event.preventDefault()}
        onClick={() => {
          requestAddToChat(formatEditorSelectionReference(selection), "plain");
          onDismiss();
        }}
        className="flex h-7 items-center gap-1.5 rounded-lg px-2 font-sans text-[13px] leading-none text-content outline-none ring-accent/40 hover:bg-content/5 focus-visible:ring-2"
      >
        <MessageSquarePlus
          aria-hidden="true"
          className="size-3.5"
          strokeWidth={1.75}
        />
        Add to chat
      </button>
    </Popover>
  );
}
