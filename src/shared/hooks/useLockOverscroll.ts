import { useCallback, useRef } from "react";

type Edges = Pick<
  HTMLElement,
  | "scrollTop"
  | "scrollLeft"
  | "clientHeight"
  | "clientWidth"
  | "scrollHeight"
  | "scrollWidth"
>;
type Delta = Pick<WheelEvent, "deltaX" | "deltaY">;

/** The scroller has run out of room in the direction the wheel is pushing. */
export function atScrollEdge(el: Edges, e: Delta): boolean {
  const canScrollX = el.scrollWidth > el.clientWidth + 1;
  const canScrollY = el.scrollHeight > el.clientHeight + 1;
  const atTop = canScrollY && el.scrollTop <= 0 && e.deltaY < 0;
  const atBottom =
    canScrollY &&
    el.scrollTop + el.clientHeight >= el.scrollHeight - 1 &&
    e.deltaY > 0;
  const atLeft = canScrollX && el.scrollLeft <= 0 && e.deltaX < 0;
  const atRight =
    canScrollX &&
    el.scrollLeft + el.clientWidth >= el.scrollWidth - 1 &&
    e.deltaX > 0;
  return atTop || atBottom || atLeft || atRight;
}

/** The scroller still has somewhere to go in the direction of the wheel. */
export function hasScrollRoom(el: Edges, e: Delta): boolean {
  if (e.deltaY < 0 && el.scrollTop > 0) return true;
  if (e.deltaY > 0 && el.scrollTop + el.clientHeight < el.scrollHeight - 1) {
    return true;
  }
  if (e.deltaX < 0 && el.scrollLeft > 0) return true;
  return e.deltaX > 0 && el.scrollLeft + el.clientWidth < el.scrollWidth - 1;
}

function scrollable(node: HTMLElement, e: Delta): boolean {
  const style = getComputedStyle(node);
  return hasScrollRoom(node, {
    deltaX: /auto|scroll|overlay/.test(style.overflowX) ? e.deltaX : 0,
    deltaY: /auto|scroll|overlay/.test(style.overflowY) ? e.deltaY : 0,
  });
}

/**
 * A scroller between the wheel's target and `el` that can still take the
 * gesture. Cancelling here would cancel it for that scroller too, since the
 * browser picks what to scroll only after the event has finished dispatching.
 */
function innerScrollerTakes(el: HTMLElement, e: WheelEvent): boolean {
  let node = e.target instanceof Element ? e.target : null;
  while (node && node !== el) {
    // Geometry first: it is free, and it rules out most of the ancestry
    // before anything has to resolve style.
    if (
      node instanceof HTMLElement &&
      hasScrollRoom(node, e) &&
      scrollable(node, e)
    )
      return true;
    node = node.parentElement;
  }
  return false;
}

/**
 * Stop macOS rubber-band bounce when a scroller reaches any edge. Returns the
 * detach, and stands alone from the hook so it can be exercised directly.
 */
export function lockOverscroll(el: HTMLElement): () => void {
  const onWheel = (e: WheelEvent) => {
    if (!atScrollEdge(el, e)) return;
    if (innerScrollerTakes(el, e)) return;
    e.preventDefault();
  };
  el.addEventListener("wheel", onWheel, { passive: false });
  return () => el.removeEventListener("wheel", onWheel);
}

export function useLockOverscroll<T extends HTMLElement>() {
  const cleanup = useRef<(() => void) | null>(null);

  return useCallback((el: T | null) => {
    cleanup.current?.();
    cleanup.current = el ? lockOverscroll(el) : null;
  }, []);
}
