import { createPortal } from "react-dom";
import type { InstalledUpdate } from "../lib/updateNotice";
import { X } from "./icons";

type Props = {
  update: InstalledUpdate | null;
  onOpen: (version: string) => void;
  onDismiss: () => void;
};

export function UpdateToastCard({ update, onOpen, onDismiss }: Props) {
  if (!update) return null;

  return (
    <section
      role="status"
      className="pointer-events-auto flex w-80 items-center gap-3 rounded-xl border border-content/15 bg-content/10 px-3 py-2.5 text-content shadow-xl backdrop-blur-xl"
    >
      <p className="min-w-0 flex-1 text-[13px] font-medium">
        MonoCode updated to {update.version}
      </p>
      <button
        type="button"
        onClick={() => onOpen(update.version)}
        className="min-h-6 shrink-0 rounded-md px-2 text-[12px] font-medium text-accent hover:bg-content/8 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
      >
        What's new
      </button>
      <button
        type="button"
        aria-label="Dismiss update notification"
        onClick={onDismiss}
        className="grid size-6 shrink-0 place-items-center rounded-md text-content/55 hover:bg-content/8 hover:text-content focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
      >
        <X className="size-3.5" strokeWidth={1.75} />
      </button>
    </section>
  );
}

export function UpdateToast(props: Props) {
  if (!props.update) return null;
  return createPortal(
    <div className="pointer-events-none fixed bottom-12 right-4 z-50">
      <UpdateToastCard {...props} />
    </div>,
    document.body,
  );
}
