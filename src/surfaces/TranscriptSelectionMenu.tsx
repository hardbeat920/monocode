import { MessageSquarePlus } from "../chrome/icons";
import { useEffect, useRef } from "react";
import { Popover } from "../chrome/Popover";
import { type TranscriptSelection } from "../lib/transcriptSelection";
import { t } from "../lib/i18n";

type Props = {
  selection: TranscriptSelection | null;
  onAddToChat: (text: string) => void;
  onDismiss: () => void;
};

export function TranscriptSelectionMenu({
  selection,
  onAddToChat,
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
      aria-label={t("Selected text actions")}
      className="p-1"
    >
      <button
        type="button"
        onMouseDown={(event) => event.preventDefault()}
        onClick={() => {
          onAddToChat(selection.text);
          window.getSelection()?.removeAllRanges();
          onDismiss();
        }}
        className="flex h-7 items-center gap-1.5 rounded-lg px-2 font-sans text-[13px] leading-none text-content outline-none ring-accent/40 hover:bg-content/5 focus-visible:ring-2"
      >
        <MessageSquarePlus
          aria-hidden="true"
          className="size-3.5"
          strokeWidth={1.75}
        />
        {t("Add to chat")}
      </button>
    </Popover>
  );
}
