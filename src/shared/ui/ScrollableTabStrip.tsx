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
  const leftWidthRef = useRef(0);
  const animationRef = useRef<number | null>(null);
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
      // Use the content's position relative to the whole row. Adding a pager
      // must not change either the destination or the visible content position.
      const max = Math.max(
        0,
        content.scrollWidth + pinnedWidth - row.clientWidth,
      );
      const position = strip.scrollLeft - leftWidthRef.current;
      const next = {
        left: max > 1 && position > 1,
        right: max > 1 && position < max - 1,
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
      if (animationRef.current !== null)
        cancelAnimationFrame(animationRef.current);
      observer.disconnect();
      strip.removeEventListener("scroll", sync);
    };
  }, [Boolean(add)]);

  useLayoutEffect(() => {
    const strip = stripRef.current;
    const width = overflowing && edges.left ? 24 : 0;
    const previous = leftWidthRef.current;
    leftWidthRef.current = width;
    if (strip && previous !== width) strip.scrollLeft += width - previous;
  }, [overflowing, edges.left]);

  const scroll = (side: "left" | "right") => {
    const row = rowRef.current;
    const strip = stripRef.current;
    const content = contentRef.current;
    if (!row || !strip || !content) return;
    if (animationRef.current !== null)
      cancelAnimationFrame(animationRef.current);
    const pinnedWidth = add
      ? compact
        ? 30
        : (addLabelRef.current?.offsetWidth ?? 0) + 16
      : 0;
    const max = Math.max(
      0,
      content.scrollWidth + pinnedWidth - row.clientWidth,
    );
    const start = Math.max(0, strip.scrollLeft - leftWidthRef.current);
    const target = Math.max(
      0,
      Math.min(
        max,
        start +
          (side === "left" ? -1 : 1) * Math.max(48, strip.clientWidth * 0.7),
      ),
    );
    const duration = window.matchMedia("(prefers-reduced-motion: reduce)")
      .matches
      ? 0
      : 200;
    const started = performance.now();
    const frame = (now: number) => {
      const progress = duration ? Math.min(1, (now - started) / duration) : 1;
      const eased = 1 - (1 - progress) ** 3;
      strip.scrollLeft =
        start + (target - start) * eased + leftWidthRef.current;
      animationRef.current = progress < 1 ? requestAnimationFrame(frame) : null;
    };
    animationRef.current = requestAnimationFrame(frame);
  };

  useLayoutEffect(() => {
    const strip = stripRef.current;
    const selected = strip?.querySelector<HTMLElement>(
      '[aria-selected="true"]',
    );
    if (!strip || !selected) return;
    if (animationRef.current !== null) {
      cancelAnimationFrame(animationRef.current);
      animationRef.current = null;
    }
    // Scroll only this row, without moving any surrounding page or pane.
    const tab = selected.getBoundingClientRect();
    const viewport = strip.getBoundingClientRect();
    if (tab.left < viewport.left) strip.scrollLeft += tab.left - viewport.left;
    else if (tab.right > viewport.right)
      strip.scrollLeft += tab.right - viewport.right;
  }, [activeKey, compact]);

  const arrow = (side: "left" | "right") =>
    edges[side] ? (
      <button
        type="button"
        aria-label={`Scroll tabs ${side}`}
        title={`Scroll tabs ${side}`}
        onClick={() => scroll(side)}
        className="grid size-6 shrink-0 place-items-center rounded-md text-content/50 hover:bg-content/5 hover:text-content"
      >
        {side === "left" ? (
          <ChevronLeft className="size-3.5" />
        ) : (
          <ChevronRight className="size-3.5" />
        )}
      </button>
    ) : null;

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
          if (animationRef.current !== null) {
            cancelAnimationFrame(animationRef.current);
            animationRef.current = null;
          }
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
