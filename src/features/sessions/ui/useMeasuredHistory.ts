import { useLayoutEffect, useRef } from "react";
import type { Block } from "../model/session";
import {
  layoutTransitionActive,
  onLayoutTransitionEnd,
} from "../../../shared/lib/layoutTransition";

export const historyHosts = new WeakMap<HTMLElement, HTMLElement>();
const detachedNodes = new WeakMap<HTMLElement, number>();

export function detachedHistoryNodes(root: HTMLElement): number {
  let count = 0;
  for (const row of root.querySelectorAll<HTMLElement>("[data-history-height]"))
    count += detachedNodes.get(row) ?? 0;
  return count;
}

/** Reveal before measuring a search result or prompt reached by keyboard. */
export function revealMeasuredTurn(element: HTMLElement) {
  const turn = element.closest<HTMLElement>("[data-history-height]");
  if (!turn) return;
  const host = historyHosts.get(turn);
  if (host) turn.appendChild(host);
  turn.removeAttribute("data-history-height");
  turn.style.removeProperty("height");
  detachedNodes.delete(turn);
}

/**
 * Keep mounted history and its exact measured geometry, but omit offscreen
 * children from layout. Never estimate heights or window the growing turn.
 * The retained geometry survives display:none tab switches. A width change,
 * edit, or late-rendered content invalidates it before the next paint.
 */
export function useMeasuredHistory(
  scroller: HTMLDivElement | null,
  visible: boolean,
  turns: Block[][],
  searching: boolean,
) {
  const previous = useRef(new Map<string, Block[]>());
  const measuredWidth = useRef(0);
  useLayoutEffect(() => {
    if (!visible || !scroller) return;
    const inner = scroller.querySelector<HTMLElement>(
      "[data-transcript-content]",
    );
    if (!inner) return;
    const rows = () =>
      [...inner.children].filter(
        (child): child is HTMLElement =>
          child instanceof HTMLElement &&
          child.hasAttribute("data-transcript-turn"),
      );
    const width = scroller.clientWidth;
    const changedWidth =
      !layoutTransitionActive() && width !== measuredWidth.current;
    if (changedWidth) measuredWidth.current = width;
    const versions = new Map(turns.map((turn) => [turn[0].id, turn]));
    for (const row of rows()) {
      const id = row.dataset.transcriptTurn!;
      if (
        searching ||
        changedWidth ||
        previous.current.get(id) !== versions.get(id)
      )
        revealMeasuredTurn(row);
    }
    previous.current = versions;
    if (searching) return;
    let frame = 0;
    const update = () => {
      frame = 0;
      // A pane sliding or being dragged changes the width every frame;
      // remeasuring the whole history each time is what makes it stutter.
      // The stale turns are offscreen, so they settle once it ends.
      if (layoutTransitionActive()) return;
      if (!scroller.isConnected || scroller.clientWidth === 0) return;
      const currentWidth = scroller.clientWidth;
      const items = rows();
      if (currentWidth !== measuredWidth.current) {
        measuredWidth.current = currentWidth;
        for (const row of items) revealMeasuredTurn(row);
      }
      const viewport = scroller.getBoundingClientRect();
      const selection = window.getSelection();
      const selecting =
        selection &&
        !selection.isCollapsed &&
        (scroller.contains(selection.anchorNode) ||
          scroller.contains(selection.focusNode));
      if (selecting) {
        for (const row of items) revealMeasuredTurn(row);
        return;
      }
      const buffer = Math.max(800, viewport.height);
      // Batch all geometry reads before applying styles.
      const measured = items.map((row) => ({
        row,
        rect: row.getBoundingClientRect(),
      }));
      for (const { row, rect } of measured) {
        const host = historyHosts.get(row);
        if (host && !observed.has(host)) {
          mutations.observe(host, {
            subtree: true,
            childList: true,
            characterData: true,
          });
          resize.observe(row);
          observed.add(host);
        }
        const outside =
          rect.bottom < viewport.top - buffer ||
          rect.top > viewport.bottom + buffer;
        if (!outside || row.classList.contains("transcript-turn-live")) {
          revealMeasuredTurn(row);
        } else if (
          rect.height > 0 &&
          !row.hasAttribute("data-history-height")
        ) {
          row.style.height = `${rect.height}px`;
          row.setAttribute("data-history-height", "");
          if (host) {
            const walker = document.createTreeWalker(host);
            let count = 1;
            while (walker.nextNode()) count++;
            detachedNodes.set(row, count);
            host.remove();
          }
        }
      }
    };
    const schedule = () => {
      if (!frame) frame = requestAnimationFrame(update);
    };
    const resize = new ResizeObserver(schedule);
    resize.observe(scroller);
    const observed = new WeakSet<HTMLElement>();
    const mutations = new MutationObserver((records) => {
      for (const record of records) {
        const target =
          record.target instanceof HTMLElement
            ? record.target
            : record.target.parentElement;
        if (target) {
          const host = target.closest<HTMLElement>("[data-history-content]");
          const row = host
            ? rows().find((row) => historyHosts.get(row) === host)
            : undefined;
          if (row) revealMeasuredTurn(row);
        }
      }
      schedule();
    });
    mutations.observe(inner, { childList: true });
    scroller.addEventListener("scroll", schedule, { passive: true });
    document.addEventListener("selectionchange", schedule);
    const invalidate = () => {
      for (const row of rows()) revealMeasuredTurn(row);
      schedule();
    };
    document.fonts?.addEventListener("loadingdone", invalidate);
    const stopTransitionEnd = onLayoutTransitionEnd(schedule);
    schedule();
    return () => {
      if (frame) cancelAnimationFrame(frame);
      stopTransitionEnd();
      resize.disconnect();
      mutations.disconnect();
      scroller.removeEventListener("scroll", schedule);
      document.removeEventListener("selectionchange", schedule);
      document.fonts?.removeEventListener("loadingdone", invalidate);
    };
  }, [scroller, visible, turns, searching]);
}
