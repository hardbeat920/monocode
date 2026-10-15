import { useEffect, useRef, useState } from "react";
import { MessageSquarePlus } from "../../../shared/ui/icons";
import { Popover } from "../../../shared/ui/Popover";
import {
  formatEditorSelectionReference,
  type EditorCodeSelection,
} from "../model/editorSelection";
import { requestAddToChat } from "../../sessions/model/quoteDraft";
import {
  composeWithChatReferences,
  newChatReference,
} from "../../sessions/model/chatReferences";
import { ReferenceNotePopover } from "../../sessions/ui/ReferenceNotePopover";

export type EditorSelectionTarget = EditorCodeSelection & {
  anchor: DOMRect;
  /** The selected passage without surrounding whitespace, and its offsets. */
  text: string;
  from: number;
  to: number;
};

/** The toolbar over selected document text, and the note box Add to chat opens. */
export function EditorSelectionMenu({
  selection,
  onDismiss,
}: {
  selection: EditorSelectionTarget | null;
  onDismiss: () => void;
}) {
  const onDismissRef = useRef(onDismiss);
  onDismissRef.current = onDismiss;
  // Writing the note moves focus out of the editor, which can drop the
  // selection; the passage being annotated is kept here instead.
  const [annotating, setAnnotating] = useState<EditorSelectionTarget | null>(
    null,
  );

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

  if (annotating) {
    return (
      <ReferenceNotePopover
        anchor={annotating.anchor}
        text={annotating.text}
        submitLabel="Add"
        onSubmit={(note) => {
          const source = {
            label: formatEditorSelectionReference(annotating),
            path: annotating.path,
            from: annotating.from,
            to: annotating.to,
          };
          const reference = newChatReference(
            annotating.text,
            note,
            undefined,
            source,
          );
          requestAddToChat(
            composeWithChatReferences("", [reference]),
            "plain",
            { text: reference.text, note: reference.note, source },
          );
          setAnnotating(null);
        }}
        onCancel={() => setAnnotating(null)}
      />
    );
  }

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
          setAnnotating(selection);
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
