import { useSyncExternalStore } from "react";
import type { SessionDropChoice } from "./sessionContext";

/** A sidebar session held over a session composer. */
export type SessionComposerHover = {
  fromId: string;
  targetId: string;
  choice: SessionDropChoice;
};

let hover: SessionComposerHover | null = null;
const listeners = new Set<() => void>();

export function setSessionComposerHover(next: SessionComposerHover | null) {
  if (
    hover?.fromId === next?.fromId &&
    hover?.targetId === next?.targetId &&
    hover?.choice === next?.choice
  ) {
    return;
  }
  hover = next;
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

function getHover() {
  return hover;
}

/** The hover for this composer, or null while nothing is held over it. */
export function useSessionComposerHover(
  sessionId: string | undefined,
): SessionComposerHover | null {
  const current = useSyncExternalStore(subscribe, getHover, getHover);
  return sessionId && current?.targetId === sessionId ? current : null;
}

/** The left half adds context, the right half links the two sessions. */
export function sessionDropChoice(
  x: number,
  rect: { left: number; width: number },
): SessionDropChoice {
  return x < rect.left + rect.width / 2 ? "context" : "link";
}

/**
 * The composer under the pointer, unless it belongs to the dragged session.
 * Returning null for the session's own composer leaves the drop to the pane.
 */
export function sessionComposerDropFromPoint(
  x: number,
  y: number,
  draggedId: string,
): { targetId: string; choice: SessionDropChoice } | null {
  if (typeof document === "undefined") return null;
  const el = document.elementFromPoint(x, y);
  const zone = el?.closest("[data-session-context-drop]") as HTMLElement | null;
  const targetId = zone?.dataset.sessionContextDrop;
  if (!zone || !targetId || targetId === draggedId) return null;
  return {
    targetId,
    choice: sessionDropChoice(x, zone.getBoundingClientRect()),
  };
}
