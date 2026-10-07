import {
  useCallback,
  useLayoutEffect,
  useRef,
  useState,
  type PointerEvent as ReactPointerEvent,
} from "react";
import { flushSync } from "react-dom";
import { setGrabbing, suppressTextSelection } from "../lib/drag";
import { reorderMotion } from "../lib/motion";
import { moveItem } from "../lib/reorder";

export type ReorderExternalDrop<T extends string> = {
  /** Return true while the pointer is over an external drop target. */
  onMove: (id: T, event: globalThis.PointerEvent) => boolean;
  /** Return true when the external target consumed the drop. */
  onDrop: (id: T, event: globalThis.PointerEvent) => boolean;
  onEnd?: (id: T) => void;
};

/**
 * Direct manipulation for a row or column of items. Items may differ in size
 * and margins; the container is assumed to space them with a uniform gap and
 * non-collapsing margins (flex or grid), so slots can be laid out from measured
 * sizes.
 */
export function useAnimatedReorder<T extends string>(
  ids: T[],
  onReorder: (ids: T[], movedId: T) => void,
  axis: "x" | "y" = "x",
  externalDrop?: ReorderExternalDrop<T>,
) {
  const nodes = useRef(new Map<T, HTMLElement>());
  const [draggingId, setDraggingId] = useState<T | null>(null);
  const latest = useRef({ ids, onReorder, externalDrop });
  useLayoutEffect(() => {
    latest.current = { ids, onReorder, externalDrop };
  }, [ids, onReorder, externalDrop]);
  const cleanup = useRef<(() => void) | null>(null);
  const finishSettling = useRef<(() => void) | null>(null);
  const suppressClickUntil = useRef(0);
  const orderKey = ids.join("\0");

  useLayoutEffect(() => () => cleanup.current?.(), [orderKey]);

  const setItemRef = useCallback((id: T, node: HTMLElement | null) => {
    if (node) nodes.current.set(id, node);
    else nodes.current.delete(id);
  }, []);

  const onItemPointerDown = useCallback(
    (id: T, event: ReactPointerEvent) => {
      if (event.button !== 0) return;
      finishSettling.current?.();
      if (cleanup.current) return;
      // A new press is a click candidate, even immediately after a previous drag.
      suppressClickUntil.current = 0;
      const items = latest.current.ids;
      const from = items.indexOf(id);
      if (items.length < 2 || from < 0) return;
      const elements = items.map((item) => nodes.current.get(item));
      if (elements.some((element) => !element)) return;
      const tabs = elements as HTMLElement[];
      // Measure once: animated positions must not affect the drop calculation.
      const rects = tabs.map((element) => {
        const rect = element.getBoundingClientRect();
        const start = axis === "x" ? rect.left : rect.top;
        const size = axis === "x" ? rect.width : rect.height;
        return { start, size, end: start + size };
      });
      // Margins travel with their item, so an item with its own trailing
      // spacing keeps it in every slot, including the last one. Whatever
      // spacing remains between the first pair is the container's gap.
      const margins = tabs.map((element) => {
        const style = window.getComputedStyle(element);
        const margin = (value: string) => parseFloat(value) || 0;
        return axis === "x"
          ? {
              before: margin(style.marginLeft),
              after: margin(style.marginRight),
            }
          : {
              before: margin(style.marginTop),
              after: margin(style.marginBottom),
            };
      });
      const gap =
        rects[1].start - rects[0].end - margins[0].after - margins[1].before;
      const origin = rects[0].start - margins[0].before;
      /** Start position of each item when the dragged item is moved to index `to`. */
      const layout = (to: number) => {
        const starts: number[] = [];
        let position = origin;
        for (const index of moveItem(
          items.map((_, i) => i),
          from,
          to,
        )) {
          position += margins[index].before;
          starts[index] = position;
          position += rects[index].size + margins[index].after + gap;
        }
        return starts;
      };
      // Offsets at which the dragged item sits exactly in each destination
      // slot. Choosing the nearest slot, rather than the nearest original item,
      // keeps every position reachable when items differ in size.
      const slots = items.map((_, to) => layout(to)[from] - rects[from].start);
      const transform = (offset: number) =>
        axis === "x"
          ? `translate3d(${offset}px, 0, 0)`
          : `translate3d(0, ${offset}px, 0)`;
      const coordinate = axis === "x" ? "clientX" : "clientY";
      const handle = tabs[from];
      const scrollProperty = axis === "x" ? "scrollLeft" : "scrollTop";
      const scrollParents: { element: HTMLElement; start: number }[] = [];
      for (
        let element = handle.parentElement;
        element;
        element = element.parentElement
      ) {
        scrollParents.push({ element, start: element[scrollProperty] });
      }
      const pointerId = event.pointerId;
      const startPosition = event[coordinate];
      const reducedMotion = window.matchMedia(
        "(prefers-reduced-motion: reduce)",
      ).matches;
      const { duration: motionDuration, easing } = reorderMotion();
      const duration = reducedMotion ? 0 : motionDuration;
      const transition = `transform ${duration}ms ${easing}`;
      let restoreSelection: (() => void) | undefined;
      let active = false;
      let settling = false;
      let destination = from;
      let frame = 0;
      let pointerPosition = startPosition;
      let overExternalTarget = false;
      let finishTimer: ReturnType<typeof setTimeout> | undefined;

      function releasePointer() {
        for (const element of tabs) delete element.dataset.reordering;
        delete handle.dataset.dragging;
        if (restoreSelection) {
          restoreSelection();
          restoreSelection = undefined;
          setGrabbing(false);
        }
        if (handle.hasPointerCapture(pointerId))
          handle.releasePointerCapture(pointerId);
      }

      function reset() {
        window.removeEventListener("pointerdown", onNextPointerDown, true);
        window.removeEventListener("pointermove", onMove);
        window.removeEventListener("pointerup", onUp);
        window.removeEventListener("pointercancel", onCancel);
        window.removeEventListener("keydown", onKey);
        window.removeEventListener("blur", onCancel);
        window.removeEventListener("scroll", onScroll, true);
        window.clearTimeout(finishTimer);
        window.cancelAnimationFrame(frame);
        for (const element of tabs) {
          element.style.removeProperty("transition");
          element.style.removeProperty("transform");
          delete element.dataset.settling;
        }
        releasePointer();
        if (active) suppressClickUntil.current = performance.now() + 400;
        latest.current.externalDrop?.onEnd?.(id);
        cleanup.current = null;
        finishSettling.current = null;
        setDraggingId(null);
      }

      /** Shifts every other item to where it sits when the dragged item occupies slot `to`. */
      function preview(to: number) {
        const starts = layout(to);
        for (let index = 0; index < tabs.length; index++) {
          if (index === from) continue;
          tabs[index].style.transform = transform(
            starts[index] - rects[index].start,
          );
        }
      }

      function paint(moveHandle = true) {
        frame = 0;
        // Scroll moves the original slots without changing their layout order.
        const scrollOffset = scrollParents.reduce(
          (offset, { element, start }) =>
            offset + element[scrollProperty] - start,
          0,
        );
        const offset = Math.max(
          slots[0],
          Math.min(
            pointerPosition - startPosition + scrollOffset,
            slots[slots.length - 1],
          ),
        );
        if (moveHandle) {
          handle.style.transform = transform(offset);
        }
        const next = slots.reduce(
          (nearest, slot, index) =>
            Math.abs(offset - slot) < Math.abs(offset - slots[nearest])
              ? index
              : nearest,
          from,
        );
        if (next !== destination) {
          destination = next;
          preview(next);
        }
      }

      function onScroll() {
        if (active && !settling && !overExternalTarget && !frame)
          frame = window.requestAnimationFrame(() => paint());
      }

      function onMove(ev: globalThis.PointerEvent) {
        if (ev.pointerId !== pointerId || settling) return;
        pointerPosition = ev[coordinate];
        if (!active) {
          if (Math.abs(pointerPosition - startPosition) < 5) return;
          active = true;
          setDraggingId(id);
          handle.setPointerCapture(pointerId);
          restoreSelection = suppressTextSelection();
          setGrabbing(true);
          for (const element of tabs) {
            element.style.transition = transition;
            element.dataset.reordering = "true";
          }
          handle.style.transition = "none";
          handle.dataset.dragging = "true";
        }
        overExternalTarget =
          latest.current.externalDrop?.onMove(id, ev) ?? false;
        if (overExternalTarget) {
          window.cancelAnimationFrame(frame);
          frame = 0;
          preview(from);
          return;
        }
        // Keep the newest position when several input events arrive in one frame.
        if (!frame) frame = window.requestAnimationFrame(() => paint());
      }

      function stop(commit: boolean, event?: globalThis.PointerEvent) {
        if (settling) return;
        if (!active) {
          reset();
          return;
        }
        if (commit && event && latest.current.externalDrop?.onDrop(id, event)) {
          reset();
          return;
        }
        settling = true;
        window.cancelAnimationFrame(frame);
        if (commit) paint(false);
        handle.dataset.settling = "true";
        releasePointer();
        suppressClickUntil.current = performance.now() + duration + 400;
        const to = commit ? destination : from;
        preview(to);
        handle.style.transition = transition;
        handle.style.transform = transform(slots[to]);
        const finish = () => {
          // Clear the preview and commit the order before the next browser paint.
          flushSync(() => {
            reset();
            if (commit && to !== from)
              latest.current.onReorder(moveItem(items, from, to), id);
          });
        };
        finishSettling.current = finish;
        if (duration === 0) finish();
        else {
          // Commit before a new press can change the list, including close buttons.
          window.addEventListener("pointerdown", onNextPointerDown, true);
          finishTimer = setTimeout(finish, duration);
        }
      }

      function onNextPointerDown() {
        finishSettling.current?.();
      }

      function onUp(ev: globalThis.PointerEvent) {
        if (ev.pointerId !== pointerId) return;
        onMove(ev);
        stop(true, ev);
      }
      function onCancel() {
        stop(false);
      }
      function onKey(ev: KeyboardEvent) {
        if (ev.key === "Escape") {
          ev.preventDefault();
          stop(false);
        }
      }

      cleanup.current = reset;
      window.addEventListener("pointermove", onMove);
      window.addEventListener("pointerup", onUp);
      window.addEventListener("pointercancel", onCancel);
      window.addEventListener("keydown", onKey);
      window.addEventListener("blur", onCancel);
      window.addEventListener("scroll", onScroll, true);
    },
    [axis],
  );

  const consumeClick = useCallback(
    () => performance.now() < suppressClickUntil.current,
    [],
  );
  return { draggingId, setItemRef, onItemPointerDown, consumeClick };
}
