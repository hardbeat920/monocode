import { useState, useSyncExternalStore } from "react";
import { ChevronDown } from "../../../shared/ui/icons";

/** Scroll events update the button without rerendering the session pane. */
export function useTranscriptJumpVisibility() {
  const [visibility] = useState(() => {
    let visible = false;
    const listeners = new Set<() => void>();
    return {
      getSnapshot: () => visible,
      subscribe: (listener: () => void) => {
        listeners.add(listener);
        return () => listeners.delete(listener);
      },
      setVisible: (next: boolean) => {
        if (visible === next) return;
        visible = next;
        for (const listener of listeners) listener();
      },
    };
  });
  return visibility;
}

export function TranscriptJumpToBottom({
  visibility,
  onJump,
}: {
  visibility: ReturnType<typeof useTranscriptJumpVisibility>;
  onJump: () => void;
}) {
  const visible = useSyncExternalStore(
    visibility.subscribe,
    visibility.getSnapshot,
    visibility.getSnapshot,
  );
  if (!visible) return null;
  return (
    <div className="pointer-events-none absolute inset-x-0 bottom-2 z-30 flex justify-center">
      <button
        type="button"
        title="Jump to latest"
        aria-label="Jump to latest"
        data-jump-to-bottom
        onClick={onJump}
        className="pointer-events-auto grid size-6 place-items-center rounded-md border border-content/15 bg-content/10 text-content shadow-md hover:bg-content/5 backdrop-blur-md"
      >
        <ChevronDown className="size-4" strokeWidth={2} />
      </button>
    </div>
  );
}
