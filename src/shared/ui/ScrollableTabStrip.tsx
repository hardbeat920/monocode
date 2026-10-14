import {
  useLayoutEffect,
  useRef,
  useState,
  type ReactNode,
  type Ref,
} from "react";
import { ChevronLeft, ChevronRight, Plus } from "./icons";
import { useLockOverscroll } from "../hooks/useLockOverscroll";

const ADD_PADDING = 16;
const COMPACT_ADD_WIDTH = 30;
const PAGER_WIDTH = 24;
const EDGE_TOLERANCE = 1;

function measureStrip(
  row: HTMLElement,
  content: HTMLElement,
  addLabel: HTMLElement | null,
) {
  const addWidth = addLabel ? addLabel.offsetWidth + ADD_PADDING : 0;
  const compact = content.scrollWidth + addWidth > row.clientWidth;
  const pinnedWidth = addWidth ? (compact ? COMPACT_ADD_WIDTH : addWidth) : 0;
  const max = Math.max(0, content.scrollWidth + pinnedWidth - row.clientWidth);
  return { compact, max };
}

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
  const hasAdd = Boolean(add);
  const [layout, setLayout] = useState({
    compact: false,
    overflowing: false,
    left: false,
    right: false,
  });
  const { compact, overflowing } = layout;
  const leftWidthRef = useRef(0);
  const animationRef = useRef<number | null>(null);

  useLayoutEffect(() => {
    const row = rowRef.current;
    const strip = stripRef.current;
    const content = contentRef.current;
    if (!row || !strip || !content) return;
    const sync = () => {
      const { compact, max } = measureStrip(row, content, addLabelRef.current);
      const position = strip.scrollLeft - leftWidthRef.current;
      const next = {
        compact,
        overflowing: max > EDGE_TOLERANCE,
        left: max > EDGE_TOLERANCE && position > EDGE_TOLERANCE,
        right: max > EDGE_TOLERANCE && position < max - EDGE_TOLERANCE,
      };
      setLayout((prev) =>
        prev.compact === next.compact &&
        prev.overflowing === next.overflowing &&
        prev.left === next.left &&
        prev.right === next.right
          ? prev
          : next,
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
  }, [hasAdd]);

  useLayoutEffect(() => {
    const strip = stripRef.current;
    const width = overflowing && layout.left ? PAGER_WIDTH : 0;
    const previous = leftWidthRef.current;
    leftWidthRef.current = width;
    if (strip && previous !== width) strip.scrollLeft += width - previous;
  }, [overflowing, layout.left]);

  const scroll = (side: "left" | "right") => {
    const row = rowRef.current;
    const strip = stripRef.current;
    const content = contentRef.current;
    if (!row || !strip || !content) return;
    if (animationRef.current !== null)
      cancelAnimationFrame(animationRef.current);
    const { max } = measureStrip(row, content, addLabelRef.current);
    const start = Math.max(0, strip.scrollLeft - leftWidthRef.current);
    const target = Math.max(
      0,
      Math.min(
        max,
        start +
          (side === "left" ? -1 : 1) *
            Math.max(PAGER_WIDTH * 2, strip.clientWidth * 0.7),
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
    layout[side] ? (
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
