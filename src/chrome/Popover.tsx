import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type ComponentPropsWithoutRef,
  type CSSProperties,
  type Ref,
} from "react";
import { createPortal } from "react-dom";
import { LAYER } from "../lib/layers";
import {
  placePopover,
  type AnchorRect,
  type PopoverAlign,
  type PopoverPosition,
  type PopoverSide,
} from "../lib/popover";

/**
 * A trigger element, a live ref to one, a rect already in viewport
 * coordinates, or a bare point for context menus.
 */
export type PopoverAnchor =
  | HTMLElement
  | { current: HTMLElement | null }
  | DOMRect
  | { x: number; y: number }
  | null;

export type PopoverDismissReason = "outside" | "escape";

type Props = Omit<ComponentPropsWithoutRef<"div">, "style"> & {
  anchor: PopoverAnchor;
  side?: PopoverSide;
  align?: PopoverAlign;
  gap?: number;
  padding?: number;
  width?: number;
  minHeight?: number;
  maxHeight?: number;
  /** Defaults to `LAYER.popover`; a flyout off an open popover wants higher. */
  layer?: number;
  /** Drops the glass frame and keeps only placement and the content animation. */
  bare?: boolean;
  style?: CSSProperties;
  autoFocus?: boolean;
  /** Wiring this in hands Popover the outside-click and Escape handling. */
  onDismiss?: (reason: PopoverDismissReason) => void;
  dismissOnEscape?: boolean;
  /** A pointer landing inside anything matching this selector is not outside. */
  ignore?: string;
  ref?: Ref<HTMLDivElement>;
};

const FRAME =
  "isolate overflow-hidden rounded-xl border border-content/10 shadow-xl";
const BACKDROP =
  "pointer-events-none absolute inset-0 z-0 bg-content/10 backdrop-blur-xl [backface-visibility:hidden] [transform:translateZ(0)]";

/** Which corner the open animation grows from, so it reads as anchored. */
function origin(side: PopoverSide, align: PopoverAlign): string {
  const near = align === "start" ? "0%" : align === "end" ? "100%" : "50%";
  if (side === "bottom") return `${near} 0%`;
  if (side === "top") return `${near} 100%`;
  return side === "right" ? `0% ${near}` : `100% ${near}`;
}

function anchorElement(anchor: PopoverAnchor): HTMLElement | null {
  if (!anchor) return null;
  if (anchor instanceof HTMLElement) return anchor;
  return "current" in anchor ? anchor.current : null;
}

function toRect(rect: DOMRect | AnchorRect): AnchorRect {
  return {
    left: rect.left,
    top: rect.top,
    right: rect.right,
    bottom: rect.bottom,
    width: rect.width,
    height: rect.height,
  };
}

function anchorRect(anchor: PopoverAnchor): AnchorRect | null {
  if (!anchor) return null;
  if (anchor instanceof HTMLElement) {
    return toRect(anchor.getBoundingClientRect());
  }
  if ("current" in anchor) {
    const el = anchor.current;
    return el ? toRect(el.getBoundingClientRect()) : null;
  }
  if ("width" in anchor) return toRect(anchor);
  const { x, y } = anchor;
  return { left: x, top: y, right: x, bottom: y, width: 0, height: 0 };
}

/** Points and rects are fresh objects every render; elements keep identity. */
function anchorKey(anchor: PopoverAnchor): unknown {
  if (!anchor || anchor instanceof HTMLElement || "current" in anchor) {
    return anchor;
  }
  if ("width" in anchor) {
    return `${anchor.left}:${anchor.top}:${anchor.width}:${anchor.height}`;
  }
  return `${anchor.x}:${anchor.y}`;
}

function samePosition(a: PopoverPosition | null, b: PopoverPosition): boolean {
  return (
    a != null &&
    a.side === b.side &&
    a.left === b.left &&
    a.top === b.top &&
    a.bottom === b.bottom &&
    a.width === b.width &&
    a.maxHeight === b.maxHeight
  );
}

/**
 * A menu, dropdown, or flyout that escapes its pane: portalled to the body so
 * no local stacking context can paint over it, placed against its anchor with
 * viewport flipping, and animated in from the anchored edge.
 */
export function Popover({
  anchor,
  side = "bottom",
  align = "start",
  gap,
  padding,
  width,
  minHeight,
  maxHeight,
  layer = LAYER.popover,
  bare = false,
  className,
  style,
  autoFocus = false,
  onDismiss,
  dismissOnEscape = true,
  ignore,
  ref,
  children,
  ...rest
}: Props) {
  const frame = useRef<HTMLDivElement | null>(null);
  const surface = useRef<HTMLDivElement | null>(null);
  const [position, setPosition] = useState<PopoverPosition | null>(null);
  const anchorRef = useRef(anchor);
  anchorRef.current = anchor;
  const dismissRef = useRef(onDismiss);
  dismissRef.current = onDismiss;
  const key = anchorKey(anchor);

  const place = useCallback(() => {
    const el = frame.current;
    const rect = anchorRect(anchorRef.current);
    if (!el || !rect) return;
    const next = placePopover(
      rect,
      { width: el.offsetWidth, height: el.offsetHeight },
      { width: window.innerWidth, height: window.innerHeight },
      { side, align, gap, padding, width, minHeight, maxHeight },
    );
    setPosition((prev) => (samePosition(prev, next) ? prev : next));
    // `key` stands in for the anchor, which is read through a ref.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, side, align, gap, padding, width, minHeight, maxHeight]);

  useLayoutEffect(() => {
    place();
    // Content that lands after open — a branch list, a filtered menu — resizes
    // the surface, and a top-anchored menu has to be measured again to sit
    // above its trigger rather than drift over it.
    const observer = new ResizeObserver(place);
    if (frame.current) observer.observe(frame.current);
    window.addEventListener("resize", place);
    window.addEventListener("scroll", place, true);
    return () => {
      observer.disconnect();
      window.removeEventListener("resize", place);
      window.removeEventListener("scroll", place, true);
    };
  }, [place]);

  useEffect(() => {
    if (autoFocus) surface.current?.focus();
  }, [autoFocus]);

  useEffect(() => {
    if (!onDismiss) return;
    const onPointerDown = (event: PointerEvent) => {
      const target = event.target as Node | null;
      if (!target) return;
      if (frame.current?.contains(target)) return;
      if (anchorElement(anchorRef.current)?.contains(target)) return;
      const el =
        target instanceof Element ? target : (target.parentElement ?? null);
      if (ignore && el?.closest(ignore)) return;
      dismissRef.current?.("outside");
    };
    const onKey = (event: KeyboardEvent) => {
      if (!dismissOnEscape || event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      dismissRef.current?.("escape");
    };
    window.addEventListener("pointerdown", onPointerDown);
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("pointerdown", onPointerDown);
      window.removeEventListener("keydown", onKey, true);
    };
  }, [onDismiss, dismissOnEscape, ignore]);

  // The first pass measures the surface off to the side; the layout effect
  // lands it before the browser paints.
  const placed: CSSProperties = position
    ? {
        position: "fixed",
        left: position.left,
        top: position.top,
        bottom: position.bottom,
        width: position.width,
        maxHeight: position.maxHeight,
      }
    : {
        position: "fixed",
        left: 0,
        top: 0,
        width,
        maxHeight: maxHeight ?? "calc(100vh - 16px)",
        visibility: "hidden",
      };

  // Keep the backdrop-filter on a stable frame. WebKit can briefly paint a
  // stale backdrop when the same composited element is transformed and then
  // invalidated by a child hover. Only this unblurred content layer moves.
  const frameInset = bare ? 0 : 2;
  const contentMaxHeight = position
    ? Math.max(0, position.maxHeight - frameInset)
    : maxHeight != null
      ? Math.max(0, maxHeight - frameInset)
      : `calc(100vh - ${16 + frameInset}px)`;

  return createPortal(
    <div
      ref={frame}
      data-popover-side={position?.side ?? side}
      style={{ ...placed, zIndex: layer }}
      className={bare ? undefined : FRAME}
    >
      {bare ? null : <div aria-hidden="true" className={BACKDROP} />}
      <div
        {...rest}
        ref={(el) => {
          surface.current = el;
          if (typeof ref === "function") ref(el);
          else if (ref) ref.current = el;
        }}
        data-popover-side={position?.side ?? side}
        style={{
          maxHeight: contentMaxHeight,
          transformOrigin: origin(position?.side ?? side, align),
          ...style,
        }}
        className={`${position ? "popover-open " : ""}relative z-[1] outline-none ${className ?? ""}`}
      >
        {children}
      </div>
    </div>,
    document.body,
  );
}
