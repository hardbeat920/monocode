import { useEffect, useLayoutEffect, type RefObject } from "react";

type Growable = {
  style: { height: string; overflowY: string };
  scrollHeight: number;
  getClientRects(): { length: number };
};

/** Fits the field to its text and scrolls only past `maxHeight`. */
export function fitTextarea(el: Growable, maxHeight: number) {
  // A hidden field has no layout box to measure; the resize observer fits it
  // once it is shown.
  if (el.getClientRects().length === 0) return;
  el.style.height = "0px";
  el.style.height = `${Math.min(el.scrollHeight, maxHeight)}px`;
  // WebKit rounds scrollHeight up at non-100% zoom, so a field that fits
  // reports a 1px overflow; only scroll once it really passes the max.
  el.style.overflowY = el.scrollHeight > maxHeight ? "auto" : "hidden";
}

/** A textarea that grows with `value` and refits when its width changes. */
export function useAutoGrow(
  ref: RefObject<HTMLTextAreaElement | null>,
  value: string,
  maxHeight = 160,
) {
  useLayoutEffect(() => {
    if (ref.current) fitTextarea(ref.current, maxHeight);
  }, [ref, value, maxHeight]);

  useEffect(() => {
    const el = ref.current;
    if (!el || typeof ResizeObserver === "undefined") return;
    // Only width changes rewrap the text; reacting to our own height changes
    // would refit for nothing.
    let width = el.clientWidth;
    const observer = new ResizeObserver(() => {
      if (el.clientWidth === width) return;
      width = el.clientWidth;
      fitTextarea(el, maxHeight);
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, [ref, maxHeight]);
}
