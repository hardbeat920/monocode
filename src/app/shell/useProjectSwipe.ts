import { useCallback, useLayoutEffect, useRef } from "react";
import { sameProjectPath } from "../../features/projects/model/recents";

type ProjectSwipeOptions = {
  cwd: string;
  projects: readonly string[];
  onSelectProject?: (path: string) => void;
};

const GESTURE_IDLE_MS = 220;
const SWIPE_DISTANCE = 30;

function reservedTarget(root: HTMLElement, target: EventTarget | null) {
  let node = target instanceof Element ? target : null;
  while (node && node !== root) {
    if (
      node.matches(
        'input, textarea, select, [contenteditable]:not([contenteditable="false"]), [role="slider"], [role="separator"]',
      )
    )
      return true;
    // Reserve horizontal scrollers for the entire gesture, even at an edge.
    if (
      node instanceof HTMLElement &&
      node.scrollWidth > node.clientWidth + 1 &&
      /auto|scroll|overlay/.test(getComputedStyle(node).overflowX)
    )
      return true;
    node = node.parentElement;
  }
  return false;
}

/** Attach once so switching projects cannot reset the momentum guard. */
export function attachProjectSwipe(
  root: HTMLElement,
  current: () => ProjectSwipeOptions,
): () => void {
  let lastEvent = -Infinity;
  let x = 0;
  let y = 0;
  let axis: "pending" | "horizontal" | "reserved" = "pending";
  let consumed = false;
  const onWheel = (event: WheelEvent) => {
    const now = performance.now();
    if (now - lastEvent > GESTURE_IDLE_MS) {
      x = y = 0;
      consumed = false;
      axis = reservedTarget(root, event.target) ? "reserved" : "pending";
    }
    lastEvent = now;
    if (event.ctrlKey || event.metaKey || event.altKey || event.shiftKey) {
      axis = "reserved";
    }
    const { cwd, projects, onSelectProject } = current();
    if (axis === "reserved" || !onSelectProject || projects.length < 2) return;
    const scale =
      event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? root.clientWidth : 1;
    x += event.deltaX * scale;
    y += event.deltaY * scale;
    if (axis === "pending") {
      if (Math.max(Math.abs(x), Math.abs(y)) < 10) return;
      axis = Math.abs(x) > Math.abs(y) * 1.5 ? "horizontal" : "reserved";
      if (axis === "reserved") return;
    }
    // Also cancel momentum at a project boundary to avoid native navigation.
    event.preventDefault();
    event.stopPropagation();
    if (consumed || Math.abs(x) < SWIPE_DISTANCE) return;
    consumed = true;
    const index = projects.findIndex((path) => sameProjectPath(path, cwd));
    if (index < 0) return;
    const next = projects[index + (x > 0 ? 1 : -1)];
    if (next) onSelectProject(next);
  };
  root.addEventListener("wheel", onWheel, { passive: false, capture: true });
  return () => root.removeEventListener("wheel", onWheel, true);
}

/** Keep the gesture listener attached across project switches, using committed options. */
export function useProjectSwipe(
  enabled: boolean,
  options: ProjectSwipeOptions,
) {
  const latest = useRef(options);
  useLayoutEffect(() => {
    latest.current = options;
  });
  const detach = useRef<(() => void) | null>(null);
  return useCallback(
    (root: HTMLElement | null) => {
      detach.current?.();
      detach.current =
        enabled && root ? attachProjectSwipe(root, () => latest.current) : null;
    },
    [enabled],
  );
}
