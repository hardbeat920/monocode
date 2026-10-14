import {
  useLayoutEffect,
  useRef,
  useState,
  type ReactNode,
  type Ref,
} from "react";
import { ChevronLeft, ChevronRight, Plus } from "./icons";
import { useLockOverscroll } from "../hooks/useLockOverscroll";

/** Natural-width tabs, with edge controls and an optional pinned add action. */
export function ScrollableTabStrip({
  children,
  label,
  activeKey,
  className = "",
  add,
}: {
  children: ReactNode;
  label: string;
  activeKey?: string;
  className?: string;
  add?: {
    label: string;
    ariaLabel: string;
    expanded: boolean;
    onClick: () => void;
    ref?: Ref<HTMLButtonElement>;
  };
}) {
  const rowRef = useRef<HTMLDivElement>(null);
  const stripRef = useRef<HTMLDivElement>(null);
  const contentRef = useRef<HTMLDivElement>(null);
  const addLabelRef = useRef<HTMLSpanElement>(null);
  const lockOverscroll = useLockOverscroll<HTMLDivElement>();
  const [compact, setCompact] = useState(false);
  const [overflowing, setOverflowing] = useState(false);
  const [edges, setEdges] = useState({ left: false, right: false });

  useLayoutEffect(() => {
    const row = rowRef.current;
    const strip = stripRef.current;
    const content = contentRef.current;
    if (!row || !strip || !content) return;
    const sync = () => {
      // Measure the full label even while it is hidden, so expanding the add
      // action never creates a resize loop at the breakpoint.
      const addWidth = addLabelRef.current
        ? addLabelRef.current.offsetWidth + 16
        : 0;
      const nextCompact = content.scrollWidth + addWidth > row.clientWidth;
      setCompact(nextCompact);
      // Decide from the row's total room, excluding the arrow slots. This
      // allows the controls to disappear as soon as the tabs fit again.
      const pinnedWidth = addWidth ? (nextCompact ? 30 : addWidth) : 0;
      setOverflowing(content.scrollWidth + pinnedWidth > row.clientWidth + 1);
      const max = strip.scrollWidth - strip.clientWidth;
      const next = {
        left: max > 1 && strip.scrollLeft > 1,
        right: max > 1 && strip.scrollLeft < max - 1,
      };
      setEdges((prev) =>
        prev.left === next.left && prev.right === next.right ? prev : next,
      );
    };
    const observer = new ResizeObserver(sync);
    observer.observe(row);
    observer.observe(strip);
    observer.observe(content);
    if (addLabelRef.current) observer.observe(addLabelRef.current);
    strip.addEventListener("scroll", sync, { passive: true });
    sync();
    return () => {
      observer.disconnect();
      strip.removeEventListener("scroll", sync);
    };
  }, [Boolean(add)]);

  useLayoutEffect(() => {
    const strip = stripRef.current;
    const selected = strip?.querySelector<HTMLElement>(
      '[aria-selected="true"]',
    );
    if (!strip || !selected) return;
    // Scroll only this row, without moving any surrounding page or pane.
    const tab = selected.getBoundingClientRect();
    const viewport = strip.getBoundingClientRect();
    if (tab.left < viewport.left) strip.scrollLeft += tab.left - viewport.left;
    else if (tab.right > viewport.right)
      strip.scrollLeft += tab.right - viewport.right;
  }, [activeKey, compact]);

  const arrow = (side: "left" | "right") => (
    <div className="w-6 shrink-0">
      {edges[side] ? (
        <button
          type="button"
          aria-label={`Scroll tabs ${side}`}
          title={`Scroll tabs ${side}`}
          onClick={() => {
            const strip = stripRef.current;
            strip?.scrollBy({
              left:
                (side === "left" ? -1 : 1) *
                Math.max(48, strip.clientWidth * 0.7),
              behavior: "smooth",
            });
          }}
          className="grid size-6 place-items-center rounded-md text-content/50 hover:bg-content/5 hover:text-content"
        >
          {side === "left" ? (
            <ChevronLeft className="size-3.5" />
          ) : (
            <ChevronRight className="size-3.5" />
          )}
        </button>
      ) : null}
    </div>
  );

  return (
    <div
      ref={rowRef}
      className={`flex min-w-0 flex-1 items-center ${className}`}
    >
      {overflowing ? arrow("left") : null}
      <div
        ref={(el) => {
          stripRef.current = el;
          lockOverscroll(el);
        }}
        role="tablist"
        aria-label={label}
        onWheel={(event) => {
          if (event.deltaX === 0 && event.deltaY !== 0 && overflowing) {
            event.currentTarget.scrollLeft += event.deltaY;
          }
        }}
        className="scrollbar-none min-w-0 flex-1 overflow-x-auto overflow-y-hidden overscroll-none"
      >
        <div ref={contentRef} className="flex w-max items-center gap-px">
          {children}
        </div>
      </div>
      {overflowing ? arrow("right") : null}
      {add ? (
        <button
          ref={add.ref}
          type="button"
          aria-label={add.ariaLabel}
          aria-haspopup="menu"
          aria-expanded={add.expanded}
          title={add.ariaLabel}
          onClick={add.onClick}
          className={`relative flex h-6 shrink-0 items-center justify-center rounded-md px-2 text-[12px] leading-none ${add.expanded ? "bg-selection text-content" : "text-content/40 hover:bg-content/5 hover:text-content"}`}
        >
          <span
            ref={addLabelRef}
            aria-hidden={compact || undefined}
            className={`flex items-center gap-1.5 whitespace-nowrap ${compact ? "pointer-events-none invisible absolute right-0" : ""}`}
          >
            <Plus className="size-3.5 shrink-0" strokeWidth={1.75} />
            {add.label}
          </span>
          {compact ? <Plus className="size-3.5" strokeWidth={1.75} /> : null}
        </button>
      ) : null}
    </div>
  );
}
