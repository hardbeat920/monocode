import { useEffect, useLayoutEffect, useRef } from "react";
import {
  editorTabKey,
  isFilesystemTab,
  type FilePaneTab,
} from "../../workspace/model/layout";

export type FileHistory = { entries: FilePaneTab[]; index: number };

export function visitFile(
  history: FileHistory,
  file: FilePaneTab,
): FileHistory {
  const current = history.entries[history.index];
  if (current && editorTabKey(current) === editorTabKey(file)) {
    const entries = [...history.entries];
    entries[history.index] = file;
    return { ...history, entries };
  }
  const entries = [...history.entries.slice(0, history.index + 1), file];
  return { entries, index: entries.length - 1 };
}

/** Horizontal scrolling belongs to the content, even when it reaches an edge. */
function horizontalScroller(target: EventTarget | null, pane: HTMLElement) {
  let node = target instanceof Element ? target : null;
  while (node && node !== pane) {
    if (
      node instanceof HTMLElement &&
      node.scrollWidth > node.clientWidth + 1
    ) {
      if (/auto|scroll|overlay/.test(getComputedStyle(node).overflowX))
        return true;
    }
    node = node.parentElement;
  }
  return false;
}

export function attachFileHistoryNavigation(
  pane: HTMLElement,
  navigate: (direction: -1 | 1) => boolean,
) {
  let lastWheel = -Infinity;
  let distance = 0;
  let consumedDirection: -1 | 0 | 1 = 0;
  let scrolling = false;
  const resetGesture = () => {
    lastWheel = -Infinity;
    distance = 0;
    consumedDirection = 0;
    scrolling = false;
  };
  const onWheel = (event: WheelEvent) => {
    const now = performance.now();
    // DOM wheel events have no gesture phase. Same-direction swipes need
    // an idle gap; otherwise they remain part of the consumed momentum stream.
    if (now - lastWheel > 250) {
      resetGesture();
    }
    lastWheel = now;
    if (
      event.ctrlKey ||
      event.metaKey ||
      event.altKey ||
      event.shiftKey ||
      Math.abs(event.deltaX) <= Math.abs(event.deltaY) * 1.5
    )
      return;
    // Remember the original target for the entire gesture: navigation replaces
    // the document under the pointer, and momentum must not navigate again.
    scrolling ||= horizontalScroller(event.target, pane);
    if (scrolling) return;
    const unit =
      event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? pane.clientWidth : 1;
    if (consumedDirection && Math.sign(event.deltaX) === consumedDirection) {
      // Same-direction events are momentum. A tiny reverse wobble must not
      // re-arm that momentum and cause another navigation.
      distance = 0;
      event.preventDefault();
      return;
    }
    distance += event.deltaX * unit;
    if (Math.abs(distance) < 80) {
      if (consumedDirection) event.preventDefault();
      return;
    }
    // A deliberate opposite swipe can start before the momentum stream ends.
    consumedDirection = distance < 0 ? -1 : 1;
    distance = 0;
    if (navigate(consumedDirection)) event.preventDefault();
  };
  pane.addEventListener("wheel", onWheel, { capture: true, passive: false });
  return () => {
    pane.removeEventListener("wheel", onWheel, true);
  };
}

/** In-memory history is unbounded and resets when this pane remounts. */
export function useFileHistory(
  activeFile: FilePaneTab | undefined,
  visible: boolean,
  onNavigate: ((file: FilePaneTab) => void) | undefined,
) {
  const element = useRef<HTMLDivElement>(null);
  const history = useRef<FileHistory>({ entries: [], index: -1 });
  const latest = useRef({ activeFile, visible, onNavigate });
  useLayoutEffect(() => {
    latest.current = { activeFile, visible, onNavigate };
    if (
      activeFile?.preview &&
      isFilesystemTab(activeFile) &&
      !activeFile.review
    ) {
      // Matching keys refresh the current visit, including history replay.
      // A different active preview records a visit and drops forward history.
      history.current = visitFile(history.current, activeFile);
    }
  }, [activeFile, visible, onNavigate]);
  useEffect(() => {
    if (!element.current) return;
    return attachFileHistoryNavigation(element.current, (direction) => {
      const view = latest.current;
      const current = history.current.entries[history.current.index];
      // Closing the preview or selecting an unrelated pinned tab suspends navigation.
      if (
        !view.visible ||
        !view.onNavigate ||
        !view.activeFile ||
        !current ||
        editorTabKey(view.activeFile) !== editorTabKey(current)
      )
        return false;
      const next = history.current.index + direction;
      const file = history.current.entries[next];
      if (!file) return false;
      history.current = { ...history.current, index: next };
      view.onNavigate(file);
      return true;
    });
  }, []);
  return element;
}
