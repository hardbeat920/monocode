import { useSyncExternalStore } from "react";
import { paneEdgeFromPoint, type PaneEdge } from "./layout";

export type PaneDrop = {
  fromId: string;
  overId: string | null;
  edge: PaneEdge;
};

export type TitleTabDropPosition = "before" | "after";

export type TitleTabDrop = {
  fromId: string;
  targetTabId: string;
  position: TitleTabDropPosition;
};

let drop: PaneDrop | null = null;
const listeners = new Set<() => void>();
let titleTabDrop: TitleTabDrop | null = null;
const titleTabListeners = new Set<() => void>();

function subscribeNoop() {
  return () => {};
}

function getNullDrop(): PaneDrop | null {
  return null;
}

export function setExternalPaneDrop(next: PaneDrop | null) {
  if (
    drop?.fromId === next?.fromId &&
    drop?.overId === next?.overId &&
    drop?.edge === next?.edge
  ) {
    return;
  }
  if (drop == null && next == null) return;
  drop = next;
  for (const listener of listeners) listener();
}

export function subscribeExternalPaneDrop(listener: () => void) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function getExternalPaneDrop() {
  return drop;
}

export function setExternalTitleTabDrop(next: TitleTabDrop | null) {
  if (
    titleTabDrop?.fromId === next?.fromId &&
    titleTabDrop?.targetTabId === next?.targetTabId &&
    titleTabDrop?.position === next?.position
  ) {
    return;
  }
  if (titleTabDrop == null && next == null) return;
  titleTabDrop = next;
  for (const listener of titleTabListeners) listener();
}

export function subscribeExternalTitleTabDrop(listener: () => void) {
  titleTabListeners.add(listener);
  return () => {
    titleTabListeners.delete(listener);
  };
}

export function getExternalTitleTabDrop() {
  return titleTabDrop;
}

/** Overlay for a drag that starts outside the pane tree (sidebar cards). */
export function useExternalPaneDrop(enabled = true) {
  return useSyncExternalStore(
    enabled ? subscribeExternalPaneDrop : subscribeNoop,
    enabled ? getExternalPaneDrop : getNullDrop,
  );
}

/** Insertion marker for a pane being detached into the title tab strip. */
export function useExternalTitleTabDrop() {
  return useSyncExternalStore(
    subscribeExternalTitleTabDrop,
    getExternalTitleTabDrop,
    getNullTitleTabDrop,
  );
}

function getNullTitleTabDrop(): TitleTabDrop | null {
  return null;
}

export function paneDropFromPoint(
  x: number,
  y: number,
): { id: string; edge: PaneEdge } | null {
  const el = document.elementFromPoint(x, y);
  const pane = el?.closest("[data-pane-id]") as HTMLElement | null;
  const id = pane?.dataset.paneId;
  if (!id || !pane) return null;
  return { id, edge: paneEdgeFromPoint(x, y, pane.getBoundingClientRect()) };
}

/** Resolve where a pane should become a new tab within the title tab strip. */
export function titleTabDropFromPoint(
  x: number,
  y: number,
): { targetTabId: string; position: TitleTabDropPosition } | null {
  const el = document.elementFromPoint(x, y);
  const strip = el?.closest("[data-title-tab-strip]") as HTMLElement | null;
  if (!strip) return null;

  const tabs = Array.from(
    strip.querySelectorAll<HTMLElement>("[data-title-tab-id]"),
  );
  if (tabs.length === 0) return null;

  const direct = el?.closest("[data-title-tab-id]") as HTMLElement | null;
  const target =
    direct && strip.contains(direct)
      ? direct
      : tabs.reduce((nearest, tab) => {
          const center =
            (tab.getBoundingClientRect().left +
              tab.getBoundingClientRect().right) /
            2;
          const nearestCenter =
            (nearest.getBoundingClientRect().left +
              nearest.getBoundingClientRect().right) /
            2;
          return Math.abs(x - center) < Math.abs(x - nearestCenter)
            ? tab
            : nearest;
        });
  const targetTabId = target.dataset.titleTabId;
  if (!targetTabId) return null;
  const rect = target.getBoundingClientRect();
  return {
    targetTabId,
    position: x < rect.left + rect.width / 2 ? "before" : "after",
  };
}
