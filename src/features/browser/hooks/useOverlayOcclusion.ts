import { useEffect, useState, type RefObject } from "react";

/** Floating UI that a native browser view would otherwise paint over. */
const OVERLAY_SELECTOR =
  '[role="dialog"], [role="alertdialog"], [role="menu"], [role="listbox"]';

type Box = { left: number; top: number; right: number; bottom: number };

export function boxesOverlap(a: Box, b: Box): boolean {
  return (
    a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom
  );
}

function overlayCovers(host: HTMLElement): boolean {
  const box = host.getBoundingClientRect();
  if (box.width <= 0 || box.height <= 0) return false;
  for (const el of document.querySelectorAll<HTMLElement>(OVERLAY_SELECTOR)) {
    if (host.contains(el)) continue;
    const rect = el.getBoundingClientRect();
    if (rect.width <= 0 || rect.height <= 0) continue;
    if (boxesOverlap(box, rect)) return true;
  }
  return false;
}

/**
 * Native webviews sit above every DOM layer, so menus, popovers and dialogs
 * that overlap the host would be drawn underneath the page. Report when that
 * happens so the caller can hide the native view until the overlay closes.
 */
export function useOverlayOcclusion(
  host: RefObject<HTMLElement | null>,
  enabled: boolean,
): boolean {
  const [covered, setCovered] = useState(false);

  useEffect(() => {
    if (!enabled) {
      setCovered(false);
      return;
    }
    let frame: number | null = null;
    const check = () => {
      frame = null;
      const el = host.current;
      setCovered(el ? overlayCovers(el) : false);
    };
    const schedule = () => {
      if (frame == null) frame = requestAnimationFrame(check);
    };
    const observer = new MutationObserver(schedule);
    observer.observe(document.body, {
      childList: true,
      subtree: true,
      attributes: true,
      attributeFilter: ["role", "class", "style", "hidden"],
    });
    window.addEventListener("resize", schedule);
    schedule();
    return () => {
      observer.disconnect();
      window.removeEventListener("resize", schedule);
      if (frame != null) cancelAnimationFrame(frame);
    };
  }, [enabled, host]);

  return covered;
}
