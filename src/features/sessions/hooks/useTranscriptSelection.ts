import { useCallback, useEffect, useRef, useState } from "react";
import {
  SELECTABLE_AGENT_RESPONSE_ATTR,
  validateTranscriptSelection,
  type TranscriptSelection,
} from "../model/transcriptSelection";

function responseIdForNode(
  node: Node | null,
  root: HTMLElement,
): string | null {
  const element = node instanceof Element ? node : node?.parentElement;
  const response = element?.closest<HTMLElement>(
    `[${SELECTABLE_AGENT_RESPONSE_ATTR}]`,
  );
  if (!response || !root.contains(response)) return null;
  return response.getAttribute(SELECTABLE_AGENT_RESPONSE_ATTR);
}

function firstRangeRect(range: Range): DOMRect | null {
  const rects = range.getClientRects();
  for (let index = 0; index < rects.length; index += 1) {
    const rect = rects.item(index);
    if (rect && (rect.width > 0 || rect.height > 0)) return rect;
  }
  const rect = range.getBoundingClientRect();
  return rect.width > 0 || rect.height > 0 ? rect : null;
}

export function useTranscriptSelection(
  root: HTMLElement | null,
  enabled: boolean,
): {
  selection: TranscriptSelection | null;
  dismissSelection: () => void;
} {
  const [selection, setSelection] = useState<TranscriptSelection | null>(null);
  const frame = useRef<number | null>(null);

  const dismissSelection = useCallback(() => setSelection(null), []);

  useEffect(() => {
    if (!enabled || !root) {
      dismissSelection();
      return;
    }

    let pointerSelecting = false;
    // Where the pointer let go; the menu opens there so it is under the hand.
    let pointerPoint: { x: number; y: number } | null = null;

    const cancelFrame = () => {
      if (frame.current == null) return;
      window.cancelAnimationFrame(frame.current);
      frame.current = null;
    };
    const reportSelection = () => {
      frame.current = null;
      const nativeSelection = window.getSelection();
      if (!nativeSelection || nativeSelection.rangeCount === 0) {
        setSelection(null);
        return;
      }
      const text = validateTranscriptSelection({
        text: nativeSelection.toString(),
        collapsed: nativeSelection.isCollapsed,
        anchorResponseId: responseIdForNode(nativeSelection.anchorNode, root),
        focusResponseId: responseIdForNode(nativeSelection.focusNode, root),
      });
      const live = text ? nativeSelection.getRangeAt(0) : null;
      const rect = live ? firstRangeRect(live) : null;
      setSelection(
        text && live && rect
          ? {
              text,
              rect: pointerPoint
                ? new DOMRect(pointerPoint.x, pointerPoint.y, 1, 1)
                : rect,
              range: live.cloneRange(),
              atPointer: pointerPoint != null,
            }
          : null,
      );
    };
    const scheduleReport = () => {
      cancelFrame();
      frame.current = window.requestAnimationFrame(reportSelection);
    };
    const onPointerDown = (event: PointerEvent) => {
      pointerPoint = null;
      pointerSelecting = responseIdForNode(event.target as Node, root) != null;
      if (pointerSelecting) cancelFrame();
    };
    const onPointerUp = (event: PointerEvent) => {
      if (!pointerSelecting) return;
      pointerSelecting = false;
      pointerPoint = { x: event.clientX, y: event.clientY };
      scheduleReport();
    };
    const onPointerCancel = () => {
      pointerSelecting = false;
    };
    const onSelectionChange = () => {
      if (!pointerSelecting) scheduleReport();
    };
    const onKeyUp = (event: KeyboardEvent) => {
      if (
        event.key === "Shift" ||
        (event.shiftKey && event.key.startsWith("Arrow"))
      ) {
        pointerPoint = null;
        scheduleReport();
      }
    };

    document.addEventListener("pointerdown", onPointerDown, { passive: true });
    document.addEventListener("pointerup", onPointerUp, { passive: true });
    document.addEventListener("pointercancel", onPointerCancel, {
      passive: true,
    });
    document.addEventListener("selectionchange", onSelectionChange);
    document.addEventListener("keyup", onKeyUp);
    return () => {
      cancelFrame();
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("pointerup", onPointerUp);
      document.removeEventListener("pointercancel", onPointerCancel);
      document.removeEventListener("selectionchange", onSelectionChange);
      document.removeEventListener("keyup", onKeyUp);
    };
  }, [dismissSelection, enabled, root]);

  return { selection, dismissSelection };
}
