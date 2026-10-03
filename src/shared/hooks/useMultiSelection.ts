import type React from "react";
import { useCallback, useLayoutEffect, useRef, useState } from "react";
import { IS_MAC } from "../../platform/tauri/platform";
import {
  applySelect,
  EMPTY_SELECTION,
  isSelected as isIdSelected,
  pruneSelection,
  selectAll as selectAllIds,
  selectionMode,
  selectionTargets,
  type MultiSelection,
  type SelectMode,
} from "../lib/multiSelection";

type ClickModifiers = { shiftKey: boolean; metaKey: boolean; ctrlKey: boolean };

export type MultiSelectionOptions = {
  visibleOrder: (scope: string) => readonly string[];
  fallbackAnchor?: (scope: string) => string | null | undefined;
};

function isEditable(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  return (
    target.isContentEditable ||
    target.closest(
      "input, textarea, select, [contenteditable]:not([contenteditable='false'])",
    ) !== null
  );
}

/**
 * Finder-style multi-selection state over `multiSelection`. Returned
 * functions are stable across renders and read the latest options, except
 * `isSelected`, which follows the rendered selection.
 */
export function useMultiSelection(options: MultiSelectionOptions) {
  const [selection, setSelection] = useState<MultiSelection>(EMPTY_SELECTION);
  const optionsRef = useRef(options);
  const selectionRef = useRef(selection);
  useLayoutEffect(() => {
    optionsRef.current = options;
    selectionRef.current = selection;
  });

  const select = useCallback(
    (id: string, scope: string | undefined, mode: SelectMode) => {
      const key = scope ?? "";
      const { visibleOrder, fallbackAnchor } = optionsRef.current;
      const order = visibleOrder(key);
      const anchor = fallbackAnchor?.(key);
      setSelection((s) =>
        applySelect(s, { id, scope: key, mode, order, fallbackAnchor: anchor }),
      );
    },
    [],
  );

  const onRowClick = useCallback(
    (event: ClickModifiers, id: string, scope?: string): SelectMode => {
      const mode = selectionMode(event);
      select(id, scope, mode);
      return mode;
    },
    [select],
  );

  // Read during render, so it follows the rendered selection (and changes
  // identity with it) rather than the ref, which lags until commit.
  const isSelected = useCallback(
    (id: string, scope?: string) => isIdSelected(selection, id, scope),
    [selection],
  );

  const targetsFor = useCallback(
    (id: string, scope?: string) =>
      selectionTargets(selectionRef.current, id, scope),
    [],
  );

  const selectAll = useCallback((scope?: string) => {
    const order = optionsRef.current.visibleOrder(scope ?? "");
    const next = selectAllIds(scope, order);
    setSelection(next);
  }, []);

  const clear = useCallback(() => setSelection(EMPTY_SELECTION), []);

  const prune = useCallback(
    (keep: (id: string) => boolean) =>
      setSelection((s) => pruneSelection(s, keep)),
    [],
  );

  const onKeyDown = useCallback(
    (event: KeyboardEvent | React.KeyboardEvent, scope?: string): boolean => {
      if (event.defaultPrevented || isEditable(event.target)) return false;
      const current = selectionRef.current;
      let acted = false;
      if (event.key === "Escape") {
        if (current.ids.length) {
          clear();
          acted = true;
        }
      } else if (
        event.key.toLowerCase() === "a" &&
        (IS_MAC ? event.metaKey : event.ctrlKey) &&
        !event.shiftKey &&
        !event.altKey
      ) {
        const target = scope ?? current.scope;
        if (target !== null) {
          selectAll(target);
          acted = true;
        }
      }
      if (acted) {
        event.preventDefault();
        event.stopPropagation();
      }
      return acted;
    },
    [clear, selectAll],
  );

  return {
    selection,
    onRowClick,
    select,
    isSelected,
    targetsFor,
    selectAll,
    clear,
    prune,
    onKeyDown,
  };
}
