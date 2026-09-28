import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { MessageSquare, TextQuote, X } from "../../../shared/ui/icons";
import { Popover } from "../../../shared/ui/Popover";
import { FileTypeIcon } from "../../files/ui/FileTypeIcon";
import type { OpenFileFn } from "../../search/model/search";
import {
  contextExcerpt,
  contextFileName,
  lineRange,
  type ChatContextItem,
} from "../model/chatContext";

const HOVER_OPEN_DELAY_MS = 220;
const HOVER_CLOSE_DELAY_MS = 100;

type Props = {
  item: ChatContextItem;
  /** Shows a remove button. Composer chips have one; sent ones do not. */
  onRemove?: () => void;
  onOpenFile?: OpenFileFn;
  /** Plays the entrance animation when the chip mounts. */
  animate?: boolean;
};

/**
 * One piece of "Add to chat" context. Hovering shows the full content. A code
 * chip opens its file at the first selected line, and a comment chip opens
 * the commented line.
 */
export function ChatContextChip({
  item,
  onRemove,
  onOpenFile,
  animate = false,
}: Props) {
  const anchor = useRef<HTMLButtonElement>(null);
  const openTimer = useRef<number | null>(null);
  const closeTimer = useRef<number | null>(null);
  const [open, setOpen] = useState(false);
  const previewId = useId();

  useEffect(
    () => () => {
      if (openTimer.current != null) window.clearTimeout(openTimer.current);
      if (closeTimer.current != null) window.clearTimeout(closeTimer.current);
    },
    [],
  );

  const clearTimers = () => {
    if (openTimer.current != null) window.clearTimeout(openTimer.current);
    if (closeTimer.current != null) window.clearTimeout(closeTimer.current);
    openTimer.current = null;
    closeTimer.current = null;
  };
  const showAfterDelay = () => {
    if (closeTimer.current != null) window.clearTimeout(closeTimer.current);
    closeTimer.current = null;
    if (open || openTimer.current != null) return;
    openTimer.current = window.setTimeout(() => {
      openTimer.current = null;
      setOpen(true);
    }, HOVER_OPEN_DELAY_MS);
  };
  const hideAfterDelay = () => {
    clearTimers();
    closeTimer.current = window.setTimeout(() => {
      closeTimer.current = null;
      setOpen(false);
    }, HOVER_CLOSE_DELAY_MS);
  };
  const hideNow = () => {
    clearTimers();
    setOpen(false);
  };

  const target = fileTarget(item);
  const openable = target != null && onOpenFile != null;
  const label = chipLabel(item);

  return (
    <>
      <span
        data-chat-context-chip={item.kind}
        className={`chat-context-chip inline-flex h-6 min-w-0 max-w-full items-center rounded-md bg-content/10 text-[11px] leading-none text-content/80 ${
          onRemove ? "pr-0.5" : ""
        } ${animate ? "chat-context-chip-enter" : ""}`}
      >
        <button
          ref={anchor}
          type="button"
          aria-label={`${label.action}: ${label.full}`}
          aria-describedby={open ? previewId : undefined}
          onMouseEnter={showAfterDelay}
          onMouseLeave={hideAfterDelay}
          onFocus={showAfterDelay}
          onBlur={hideNow}
          onClick={(event) => {
            event.stopPropagation();
            if (openable) {
              hideNow();
              onOpenFile(target.path, { line: target.line });
              return;
            }
            clearTimers();
            setOpen(true);
          }}
          className={`flex h-full min-w-0 items-center gap-1.5 rounded-md pl-1.5 outline-none focus-visible:ring-2 focus-visible:ring-accent/60 ${
            onRemove ? "pr-1" : "pr-2"
          } ${openable ? "hover:text-content" : "cursor-default"}`}
        >
          <ChipIcon item={item} />
          {label.body}
        </button>
        {onRemove ? (
          <button
            type="button"
            title="Remove"
            aria-label={`Remove ${label.full}`}
            onClick={(event) => {
              event.stopPropagation();
              hideNow();
              onRemove();
            }}
            className="grid size-4 shrink-0 place-items-center rounded-full text-content/40 hover:bg-content/15 hover:text-content"
          >
            <X className="size-3" strokeWidth={2} />
          </button>
        ) : null}
      </span>
      {open ? (
        <Popover
          anchor={anchor}
          side="top"
          align="start"
          gap={6}
          width={item.kind === "code" ? 280 : 360}
          maxHeight={320}
          onDismiss={hideNow}
          id={previewId}
          role="tooltip"
          data-chat-context-preview={item.kind}
          className="p-3 font-sans text-content"
          onMouseEnter={() => {
            if (closeTimer.current != null) {
              window.clearTimeout(closeTimer.current);
              closeTimer.current = null;
            }
          }}
          onMouseLeave={hideAfterDelay}
        >
          <ChatContextPreview item={item} openable={openable} />
        </Popover>
      ) : null}
    </>
  );
}

function ChipIcon({ item }: { item: ChatContextItem }) {
  if (item.kind === "code") {
    return (
      <span className="grid size-3.5 shrink-0 place-items-center">
        <FileTypeIcon
          name={contextFileName(item.path)}
          isDir={false}
          size={14}
        />
      </span>
    );
  }
  const Icon = item.kind === "quote" ? TextQuote : MessageSquare;
  return (
    <Icon
      aria-hidden="true"
      className="size-3.5 shrink-0 text-content/45"
      strokeWidth={1.75}
    />
  );
}

function chipLabel(item: ChatContextItem): {
  action: string;
  full: string;
  body: ReactNode;
} {
  if (item.kind === "quote") {
    const excerpt = contextExcerpt(item.text);
    return {
      action: "Quoted text",
      full: excerpt,
      body: <span className="min-w-0 max-w-56 truncate">{excerpt}</span>,
    };
  }

  const name = contextFileName(item.path);
  if (item.kind === "code") {
    const lines = lineRange(item.startLine, item.endLine);
    return {
      action: "Open selected lines",
      full: `${item.path}, lines ${lines}`,
      body: (
        <>
          <span className="min-w-0 max-w-44 truncate">{name}</span>
          <LineTag>L{lines}</LineTag>
        </>
      ),
    };
  }

  const comment = contextExcerpt(item.comment);
  return {
    action: "Comment",
    full: `${item.line != null ? `${item.path}:${item.line}` : item.path}, ${comment}`,
    body: (
      <>
        <span className="max-w-32 shrink-0 truncate">{name}</span>
        {item.line != null ? <LineTag>L{item.line}</LineTag> : null}
        <span className="min-w-0 max-w-48 truncate text-content/55">
          {comment}
        </span>
      </>
    ),
  };
}

function LineTag({ children }: { children: ReactNode }) {
  return (
    <span className="shrink-0 font-mono text-[10px] tabular-nums text-content/45">
      {children}
    </span>
  );
}

function ChatContextPreview({
  item,
  openable,
}: {
  item: ChatContextItem;
  openable: boolean;
}) {
  if (item.kind === "quote") {
    return (
      <>
        <PreviewHeader item={item} title="Quoted text" />
        <p className="mt-2 whitespace-pre-wrap break-words border-l-2 border-content/15 pl-2.5 text-[12px] leading-5 text-content/80">
          {item.text}
        </p>
      </>
    );
  }

  if (item.kind === "code") {
    const lines = lineRange(item.startLine, item.endLine);
    return (
      <>
        <PreviewHeader
          item={item}
          title={item.endLine > item.startLine ? `Lines ${lines}` : `Line ${lines}`}
        />
        <p className="mt-1.5 break-all font-mono text-[11px] leading-4 text-content/55">
          {item.path}
        </p>
        {openable ? (
          <p className="mt-2 text-[11px] text-content/40">Click to open</p>
        ) : null}
      </>
    );
  }

  const marker =
    item.change === "added" ? "+" : item.change === "removed" ? "-" : " ";
  const tone =
    item.change === "added"
      ? "bg-emerald-500/15 text-emerald-300"
      : item.change === "removed"
        ? "bg-rose-500/15 text-rose-300"
        : "bg-content/6 text-content/70";
  return (
    <>
      <PreviewHeader
        item={item}
        title={item.line != null ? `${item.path}:${item.line}` : item.path}
      />
      <pre
        className={`mt-2 overflow-x-auto whitespace-pre rounded-md px-2 py-1 font-mono text-[11px] leading-4 ${tone}`}
      >
        {marker} {item.code}
      </pre>
      <p className="mt-2 whitespace-pre-wrap break-words text-[12px] leading-5 text-content/85">
        {item.comment}
      </p>
    </>
  );
}

function PreviewHeader({
  item,
  title,
}: {
  item: ChatContextItem;
  title: string;
}) {
  return (
    <div className="flex min-w-0 items-center gap-1.5 text-[11px] text-content/50">
      <ChipIcon item={item} />
      <span className="min-w-0 truncate">{title}</span>
    </div>
  );
}

function fileTarget(
  item: ChatContextItem,
): { path: string; line: number } | null {
  if (item.kind === "code") return { path: item.path, line: item.startLine };
  if (item.kind === "comment" && item.line != null && item.change !== "removed") {
    return { path: item.path, line: item.line };
  }
  return null;
}
