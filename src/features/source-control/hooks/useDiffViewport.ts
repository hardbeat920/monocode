import {
  useCallback,
  useLayoutEffect,
  useRef,
  useState,
  type RefObject,
} from "react";
import { UNIFIED_OVERSCAN_PX } from "../model/unifiedDiffWindow";

export type DiffViewport = { top: number; bottom: number };

/** How far the viewport's bottom edge may move before React re-renders. The
 * row window's overscan already mounts rows past it, so a window drag that
 * only changes the height commits once per this many pixels, not per frame. */
export const DIFF_VIEWPORT_SLACK_PX = UNIFIED_OVERSCAN_PX / 3;

/** A single observer and scroll subscription for the whole review. Horizontal
 * movement stays native and does not trigger a React render. */
export function useDiffViewport(
  content: RefObject<HTMLDivElement | null>,
  scroller: RefObject<HTMLDivElement | null>,
  fill: boolean,
  enabled: boolean,
) {
  const [viewport, setViewport] = useState<DiffViewport>({
    top: 0,
    bottom: 768,
  });
  // `current` is exact; `rendered` is what the last committed window used.
  const current = useRef(viewport);
  const rendered = useRef(viewport);
  const root = useRef<HTMLElement | null>(null);
  const height = useRef(0);
  const measure = useCallback(() => {
    const body = content.current;
    const surface = scroller.current;
    if (!body || !surface) return;
    root.current = fill ? surface : verticalScrollParent(surface);
    // The filled review starts at the scrollport's origin. Reading its scroll
    // offset avoids walking layout just to subtract two identical origins.
    const top = fill
      ? surface.scrollTop
      : (root.current?.getBoundingClientRect().top ?? 0) -
        body.getBoundingClientRect().top;
    const viewHeight = fill
      ? height.current || surface.clientHeight || window.innerHeight || 768
      : root.current?.clientHeight || window.innerHeight || 768;
    if (fill) height.current = viewHeight;
    const next = {
      top,
      bottom: top + viewHeight,
    };
    const previous = current.current;
    if (previous.top === next.top && previous.bottom === next.bottom) return;
    current.current = next;
    const shown = rendered.current;
    if (
      shown.top === next.top &&
      Math.abs(shown.bottom - next.bottom) < DIFF_VIEWPORT_SLACK_PX
    )
      return;
    rendered.current = next;
    setViewport(next);
  }, [content, fill, scroller]);

  useLayoutEffect(() => {
    if (!enabled || !content.current || !scroller.current) return;
    height.current = 0;
    measure();
    const target = root.current ?? window;
    let frame = 0;
    const schedule = () => {
      if (frame) return;
      frame = requestAnimationFrame(() => {
        frame = 0;
        measure();
      });
    };
    target.addEventListener("scroll", schedule, { passive: true });
    if (!fill) window.addEventListener("resize", schedule);
    const observer = new ResizeObserver((entries) => {
      if (!fill) {
        schedule();
        return;
      }
      // Width-only drags are handled entirely by CSS. ResizeObserver already
      // supplies the height after layout; do not queue a geometry read in the
      // following frame, where it could flush the next width write.
      const resized = entries.find(
        (entry) => entry.target === scroller.current,
      );
      if (!resized) return;
      const nextHeight = Math.round(resized.contentRect.height);
      if (nextHeight <= 0 || height.current === nextHeight) return;
      height.current = nextHeight;
      measure();
    });
    observer.observe(scroller.current);
    if (!fill) {
      observer.observe(content.current);
      if (root.current && root.current !== scroller.current)
        observer.observe(root.current);
    }
    return () => {
      target.removeEventListener("scroll", schedule);
      window.removeEventListener("resize", schedule);
      observer.disconnect();
      if (frame) cancelAnimationFrame(frame);
    };
  }, [content, enabled, fill, measure, scroller]);

  const scrollTo = useCallback(
    (top: number) => {
      const body = content.current;
      if (!body) return;
      if (fill && scroller.current)
        scroller.current.scrollTop = Math.max(0, top);
      else if (root.current)
        root.current.scrollTop +=
          body.getBoundingClientRect().top -
          root.current.getBoundingClientRect().top +
          top;
      else window.scrollBy({ top: body.getBoundingClientRect().top + top });
      measure();
    },
    [content, fill, measure, scroller],
  );
  return { viewport, current, scrollTo, measure };
}

function verticalScrollParent(element: HTMLElement): HTMLElement | null {
  let parent = element.parentElement;
  while (parent) {
    if (/^(auto|scroll)$/.test(getComputedStyle(parent).overflowY))
      return parent;
    parent = parent.parentElement;
  }
  return null;
}
