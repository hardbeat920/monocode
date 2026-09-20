import {
  useCallback,
  useRef,
  useState,
  type PointerEvent as ReactPointerEvent,
} from "react";
import { setGrabbing, suppressTextSelection } from "../lib/drag";
import { moveItem } from "../lib/reorder";

const THRESHOLD = 5;
const DROP_ON_INSET = 0.25;

export type SortableDropTarget = {
  kind: "tab" | "group";
  id: string;
  /** False when the drop is refused — the target is flagged, not acted on. */
  allowed: boolean;
};

export type SortableOptions = {
  axis?: "x" | "y";
  /** Called once the pointer crosses the drag threshold. */
  onActivate?: (id: string) => void;
  onDropOnItem?: (draggedId: string, targetId: string) => void;
  onDropOnGroup?: (draggedId: string, groupId: string) => void;
  canDropOn?: (
    draggedId: string,
    kind: "tab" | "group",
    targetId: string,
  ) => boolean;
};

type DragState = {
  id: string;
  pointerId: number;
  startX: number;
  startY: number;
  active: boolean;
  toIndex: number;
  dropTarget: SortableDropTarget | null;
};

function optionsOf(
  axisOrOptions: "x" | "y" | SortableOptions | undefined,
): SortableOptions {
  if (axisOrOptions == null) return { axis: "x" };
  if (axisOrOptions === "x" || axisOrOptions === "y") return { axis: axisOrOptions };
  return { axis: "x", ...axisOrOptions };
}

export function useSortable(
  ids: string[],
  onReorder: (ids: string[], movedId?: string) => void,
  axisOrOptions: "x" | "y" | SortableOptions = "x",
) {
  const options = optionsOf(axisOrOptions);
  const axis = options.axis ?? "x";
  const idsRef = useRef(ids);
  idsRef.current = ids;
  const onReorderRef = useRef(onReorder);
  onReorderRef.current = onReorder;
  const onDropOnItemRef = useRef(options.onDropOnItem);
  onDropOnItemRef.current = options.onDropOnItem;
  const onDropOnGroupRef = useRef(options.onDropOnGroup);
  onDropOnGroupRef.current = options.onDropOnGroup;
  const onActivateRef = useRef(options.onActivate);
  onActivateRef.current = options.onActivate;
  const canDropOnRef = useRef(options.canDropOn);
  canDropOnRef.current = options.canDropOn;
  const nodes = useRef(new Map<string, HTMLElement>());
  const groupNodes = useRef(new Map<string, HTMLElement>());
  const drag = useRef<DragState | null>(null);
  const suppressClickUntil = useRef(0);

  const [draggingId, setDraggingId] = useState<string | null>(null);
  const [toIndex, setToIndex] = useState<number | null>(null);
  const [dropTarget, setDropTarget] = useState<SortableDropTarget | null>(null);

  const setItemRef = useCallback((id: string, el: HTMLElement | null) => {
    if (el) nodes.current.set(id, el);
    else nodes.current.delete(id);
  }, []);

  const setGroupDropRef = useCallback((id: string, el: HTMLElement | null) => {
    if (el) groupNodes.current.set(id, el);
    else groupNodes.current.delete(id);
  }, []);

  const indexAt = useCallback(
    (x: number, y: number) => {
      const list = idsRef.current;
      const pos = axis === "x" ? x : y;
      let next = list.length - 1;
      for (let i = 0; i < list.length; i++) {
        const rect = nodes.current.get(list[i])?.getBoundingClientRect();
        if (!rect) continue;
        const mid =
          axis === "x" ? rect.left + rect.width / 2 : rect.top + rect.height / 2;
        if (pos < mid) {
          next = i;
          break;
        }
      }
      return next;
    },
    [axis],
  );

  const dropTargetAt = useCallback(
    (draggedId: string, x: number, y: number): SortableDropTarget | null => {
      const target = (
        kind: "tab" | "group",
        id: string,
      ): SortableDropTarget => ({
        kind,
        id,
        allowed: canDropOnRef.current?.(draggedId, kind, id) ?? true,
      });
      for (const [groupId, el] of groupNodes.current) {
        const rect = el.getBoundingClientRect();
        if (x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom) {
          return target("group", groupId);
        }
      }
      if (!onDropOnItemRef.current) return null;
      for (const id of idsRef.current) {
        if (id === draggedId) continue;
        const rect = nodes.current.get(id)?.getBoundingClientRect();
        if (!rect) continue;
        const inset =
          axis === "x" ? rect.width * DROP_ON_INSET : rect.height * DROP_ON_INSET;
        const inCenter =
          axis === "x"
            ? x >= rect.left + inset &&
              x <= rect.right - inset &&
              y >= rect.top &&
              y <= rect.bottom
            : y >= rect.top + inset &&
              y <= rect.bottom - inset &&
              x >= rect.left &&
              x <= rect.right;
        if (inCenter) return target("tab", id);
      }
      return null;
    },
    [axis],
  );

  const onItemPointerDown = useCallback(
    (id: string, event: ReactPointerEvent) => {
      if (event.button !== 0) return;
      if (idsRef.current.length < 2) return;
      if ((event.target as HTMLElement | null)?.closest("[data-no-drag]")) {
        return;
      }
      const handle = event.currentTarget as HTMLElement;
      const from = idsRef.current.indexOf(id);
      if (from < 0) return;

      const state: DragState = {
        id,
        pointerId: event.pointerId,
        startX: event.clientX,
        startY: event.clientY,
        active: false,
        toIndex: from,
        dropTarget: null,
      };
      drag.current = state;
      handle.setPointerCapture(event.pointerId);
      const restoreSelection = suppressTextSelection();

      const onMove = (ev: PointerEvent) => {
        const current = drag.current;
        if (!current || current.id !== id) return;
        if (!current.active) {
          if (
            Math.hypot(ev.clientX - current.startX, ev.clientY - current.startY) <
            THRESHOLD
          ) {
            return;
          }
          current.active = true;
          setGrabbing(true);
          setDraggingId(id);
          setToIndex(from);
          onActivateRef.current?.(id);
        }
        const dropOn = dropTargetAt(id, ev.clientX, ev.clientY);
        const next = indexAt(ev.clientX, ev.clientY);
        if (
          next === current.toIndex &&
          current.dropTarget?.kind === dropOn?.kind &&
          current.dropTarget?.id === dropOn?.id &&
          current.dropTarget?.allowed === dropOn?.allowed
        ) {
          return;
        }
        current.toIndex = next;
        current.dropTarget = dropOn;
        setToIndex(next);
        setDropTarget(dropOn);
      };

      const onUp = () => stop(true);
      const onKey = (ev: KeyboardEvent) => {
        if (ev.key === "Escape") {
          ev.preventDefault();
          stop(false);
        }
      };

      function stop(commit: boolean) {
        window.removeEventListener("pointermove", onMove);
        window.removeEventListener("pointerup", onUp);
        window.removeEventListener("pointercancel", onUp);
        window.removeEventListener("keydown", onKey);
        const current = drag.current;
        drag.current = null;
        restoreSelection();
        setGrabbing(false);
        setDraggingId(null);
        setToIndex(null);
        setDropTarget(null);
        try {
          handle.releasePointerCapture(state.pointerId);
        } catch {
          /* already released */
        }
        if (!current?.active) return;
        if (!commit) return;
        suppressClickUntil.current = performance.now() + 400;
        if (current.dropTarget) {
          // A refused target still swallows the drop: the tab stays put rather
          // than reordering into a group it cannot join.
          if (!current.dropTarget.allowed) return;
          if (current.dropTarget.kind === "tab") {
            onDropOnItemRef.current?.(current.id, current.dropTarget.id);
          } else {
            onDropOnGroupRef.current?.(current.id, current.dropTarget.id);
          }
          return;
        }
        const list = idsRef.current;
        const fromIndex = list.indexOf(current.id);
        if (fromIndex >= 0 && current.toIndex !== fromIndex) {
          onReorderRef.current(moveItem(list, fromIndex, current.toIndex), current.id);
        }
      }

      window.addEventListener("pointermove", onMove);
      window.addEventListener("pointerup", onUp);
      window.addEventListener("pointercancel", onUp);
      window.addEventListener("keydown", onKey);
    },
    [dropTargetAt, indexAt],
  );

  const consumeClick = useCallback(
    () => performance.now() < suppressClickUntil.current,
    [],
  );

  return {
    draggingId,
    fromIndex: draggingId ? ids.indexOf(draggingId) : null,
    toIndex: draggingId && !dropTarget ? toIndex : null,
    dropTarget: draggingId ? dropTarget : null,
    setItemRef,
    setGroupDropRef,
    onItemPointerDown,
    consumeClick,
  };
}
