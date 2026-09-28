import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
} from "react";
import { ChevronDown, ChevronUp, Search, X } from "../../../shared/ui/icons";
import type { Block } from "../model/session";
import { findTranscriptBlocks } from "../model/transcriptFind";
import { keybindingPressed } from "../../settings/model/settings";

type Props = {
  blocks: Block[];
  visible: boolean;
  focused: boolean;
  onNavigate: (blockId: string | null, query?: string) => boolean;
  side?: "left" | "right";
};

export function TranscriptFind({
  blocks,
  visible,
  focused,
  onNavigate,
  side = "right",
}: Props) {
  const input = useRef<HTMLInputElement>(null);
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const matches = useMemo(
    () => findTranscriptBlocks(blocks, query),
    [blocks, query],
  );
  const selected = matches[Math.min(active, matches.length - 1)] ?? null;

  const openFind = () => {
    setOpen(true);
    requestAnimationFrame(() => {
      input.current?.focus();
      input.current?.select();
    });
  };
  const closeFind = () => {
    setOpen(false);
    onNavigate(null);
  };
  const step = (direction: number) => {
    if (!matches.length) return;
    setActive((index) => (index + direction + matches.length) % matches.length);
  };

  useEffect(() => {
    if (!open || !visible) return;
    const frame = requestAnimationFrame(() => onNavigate(selected, query));
    return () => cancelAnimationFrame(frame);
  }, [open, visible, selected, query, onNavigate]);

  useEffect(() => {
    if (!visible || !focused) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.isComposing) return;
      const target = event.target instanceof Element ? event.target : null;
      if (
        target?.closest(
          ".cm-editor, .monocode-terminal, [role='dialog'], [data-app-search]",
        )
      )
        return;
      const mod = event.metaKey || event.ctrlKey;
      const defaultFind =
        mod &&
        !event.altKey &&
        !event.shiftKey &&
        event.key.toLowerCase() === "f";
      if (keybindingPressed("Editor: Find", event, defaultFind)) {
        event.preventDefault();
        event.stopPropagation();
        openFind();
      } else if (defaultFind) {
        event.preventDefault();
        event.stopPropagation();
      } else if (
        open &&
        (event.key === "F3" ||
          (mod && !event.altKey && event.key.toLowerCase() === "g"))
      ) {
        event.preventDefault();
        event.stopPropagation();
        step(event.shiftKey ? -1 : 1);
      } else if (open && event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        closeFind();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  });

  if (!visible || !open) return null;

  return (
    <div
      className={`pointer-events-none absolute top-2 z-40 ${side === "left" ? "left-3" : "right-3"}`}
    >
      <div
        role="search"
        aria-label="Find in conversation"
        className="pointer-events-auto flex w-[min(360px,calc(100cqw-24px))] items-center gap-1 rounded-lg border border-content/10 bg-content/5 p-1 shadow-lg backdrop-blur-xl"
      >
        <Search
          className="ml-1 size-3.5 shrink-0 text-content/50"
          strokeWidth={1.75}
        />
        <input
          ref={input}
          type="text"
          value={query}
          aria-label="Find in conversation"
          placeholder="Find in conversation"
          autoComplete="off"
          autoCorrect="off"
          autoCapitalize="off"
          spellCheck={false}
          onChange={(event) => {
            setQuery(event.target.value);
            setActive(0);
          }}
          onKeyDown={(event: ReactKeyboardEvent<HTMLInputElement>) => {
            if (event.key === "Enter") {
              event.preventDefault();
              step(event.shiftKey ? -1 : 1);
            }
          }}
          className="w-44 min-w-0 flex-1 bg-transparent text-[12px] text-content outline-none placeholder:text-content/40"
        />
        <span
          aria-live="polite"
          className="min-w-[10ch] shrink-0 whitespace-nowrap text-right font-mono text-[11px] tabular-nums text-content/50"
        >
          {query.trim()
            ? matches.length
              ? `${Math.min(active, matches.length - 1) + 1} of ${matches.length}`
              : "No results"
            : ""}
        </span>
        <FindButton
          label="Previous match"
          onClick={() => step(-1)}
          disabled={!matches.length}
        >
          <ChevronUp className="size-3.5" strokeWidth={1.75} />
        </FindButton>
        <FindButton
          label="Next match"
          onClick={() => step(1)}
          disabled={!matches.length}
        >
          <ChevronDown className="size-3.5" strokeWidth={1.75} />
        </FindButton>
        <FindButton label="Close find" onClick={closeFind}>
          <X className="size-3.5" strokeWidth={1.75} />
        </FindButton>
      </div>
    </div>
  );
}

function FindButton({
  label,
  onClick,
  disabled,
  children,
}: {
  label: string;
  onClick: () => void;
  disabled?: boolean;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      disabled={disabled}
      onClick={onClick}
      className="grid size-6 place-items-center rounded text-content/55 hover:bg-content/10 hover:text-content disabled:opacity-30"
    >
      {children}
    </button>
  );
}
