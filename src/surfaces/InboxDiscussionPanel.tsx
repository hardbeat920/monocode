import { useEffect, useRef, useState } from "react";
import { PanelLeft, RotateCcw } from "../chrome/icons";
import { IconButton } from "../chrome/TitleBar";
import { useDragResize } from "../hooks/useDragResize";
import { inboxItemRef, type InboxItem } from "../lib/githubTasks";
import { inboxAskKey } from "../lib/inboxAsk";

export type InboxSessionPortal = { sessionId: string; host: HTMLElement };
let rememberedWidth = 440;

/** Only a destination for the regular session pane; it owns no chat state. */
export function InboxDiscussionPanel({
  item,
  onClose,
  onOpen,
  onRestart,
  onMount,
}: {
  item: InboxItem;
  onClose: () => void;
  onOpen: (item: InboxItem) => Promise<string>;
  onRestart: (item: InboxItem) => Promise<string>;
  onMount: (portal: InboxSessionPortal | null) => void;
}) {
  const host = useRef<HTMLDivElement>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const resize = useDragResize({
    min: 360,
    max: () => Math.max(360, window.innerWidth - 650),
    initial: rememberedWidth,
    defaultWidth: 440,
    direction: "left",
    onCommit: (width) => {
      rememberedWidth = width;
    },
  });
  const key = inboxAskKey(item);
  useEffect(() => {
    let active = true;
    setError(null);
    setLoading(true);
    void onOpen(item)
      .then((sessionId) => {
        if (active && host.current) onMount({ sessionId, host: host.current });
      })
      .catch((reason) => {
        if (active) setError(String(reason));
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
      onMount(null);
    };
    // Metadata refreshes for the same item must not remount its composer.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, onOpen, onMount]);

  return (
    <aside
      ref={resize.setPaneRef}
      aria-label={`Ask about ${inboxItemRef(item)}`}
      className="relative flex min-h-0 shrink-0 flex-col border-l border-content/10 max-[1100px]:absolute max-[1100px]:inset-0 max-[1100px]:z-10 max-[1100px]:!w-auto"
    >
      <div
        role="separator"
        aria-label="Resize discussion"
        aria-orientation="vertical"
        onPointerDown={resize.onPointerDown}
        onDoubleClick={resize.onDoubleClick}
        className="absolute inset-y-0 -left-1 z-20 w-2 cursor-col-resize max-[1100px]:hidden"
      />
      <header className="flex h-11 shrink-0 items-center border-b border-content/10 px-3">
        <span className="min-w-0 flex-1 truncate text-[13px] font-medium">
          Ask · {inboxItemRef(item)}
        </span>
        <IconButton
          label="Restart conversation"
          disabled={loading}
          onClick={() => {
            setLoading(true);
            setError(null);
            void onRestart(item)
              .then(sessionId => {
                if (host.current) onMount({ sessionId, host: host.current });
              })
              .catch(reason => setError(String(reason)))
              .finally(() => setLoading(false));
          }}
        >
          <RotateCcw className="size-3.5" />
        </IconButton>
        <IconButton label="Close panel" onClick={onClose}>
          <PanelLeft className="size-3.5" />
        </IconButton>
      </header>
      {error ? (
        <p role="alert" className="p-3 text-xs text-red-400">
          {error}
        </p>
      ) : null}
      <div ref={host} className="flex min-h-0 min-w-0 flex-1 flex-col" />
    </aside>
  );
}
