import { IS_MAC } from "../../platform/tauri/platform";

/** Finder-style multi-selection over opaque ids, confined to one scope. */
export type MultiSelection = {
  /** Group the selection lives in (e.g. "staged" / "unstaged"); null when empty. */
  scope: string | null;
  /** Selected ids. */
  ids: string[];
  /** Where the next range starts. */
  anchor: string | null;
};

export const EMPTY_SELECTION: MultiSelection = {
  scope: null,
  ids: [],
  anchor: null,
};

export type SelectMode = "single" | "toggle" | "range";

/** Shift → range; Cmd on macOS / Ctrl elsewhere → toggle; else single. */
export function selectionMode(
  event: { shiftKey: boolean; metaKey: boolean; ctrlKey: boolean },
  isMac: boolean = IS_MAC,
): SelectMode {
  if (event.shiftKey) return "range";
  if (isMac ? event.metaKey : event.ctrlKey) return "toggle";
  return "single";
}

/**
 * Applies a click to the selection. `order` is the scope's visible ids in
 * on-screen order; `fallbackAnchor` starts a range when there's no anchor.
 */
export function applySelect(
  selection: MultiSelection,
  click: {
    id: string;
    scope?: string;
    mode: SelectMode;
    order: readonly string[];
    fallbackAnchor?: string | null;
  },
): MultiSelection {
  const { id, scope = "", mode, order, fallbackAnchor } = click;
  const sameScope = selection.scope === scope;
  if (mode === "toggle") {
    const ids = sameScope ? selection.ids : [];
    return {
      scope,
      ids: ids.includes(id) ? ids.filter((i) => i !== id) : [...ids, id],
      anchor: id,
    };
  }
  if (mode === "range") {
    const anchor =
      sameScope && selection.anchor && order.includes(selection.anchor)
        ? selection.anchor
        : fallbackAnchor && order.includes(fallbackAnchor)
          ? fallbackAnchor
          : null;
    const from = anchor ? order.indexOf(anchor) : -1;
    const to = order.indexOf(id);
    if (from >= 0 && to >= 0) {
      const [start, end] = from <= to ? [from, to] : [to, from];
      return { scope, ids: order.slice(start, end + 1), anchor };
    }
  }
  return { scope, ids: [id], anchor: id };
}

/** Keeps only ids (and the anchor) for which `keep` holds. */
export function pruneSelection(
  selection: MultiSelection,
  keep: (id: string) => boolean,
): MultiSelection {
  const ids = selection.ids.filter(keep);
  const anchor =
    selection.anchor !== null && keep(selection.anchor)
      ? selection.anchor
      : null;
  if (ids.length === selection.ids.length && anchor === selection.anchor) {
    return selection;
  }
  return ids.length || anchor !== null
    ? { scope: selection.scope, ids, anchor }
    : EMPTY_SELECTION;
}

export function isSelected(
  selection: MultiSelection,
  id: string,
  scope = "",
): boolean {
  return selection.scope === scope && selection.ids.includes(id);
}

/** The ids an action on `id` applies to: the selection if it holds `id`, else just `id`. */
export function selectionTargets(
  selection: MultiSelection,
  id: string,
  scope = "",
): string[] {
  return isSelected(selection, id, scope) ? [...selection.ids] : [id];
}

/** Selects every id in `order`, anchored on the first. */
export function selectAll(
  scope: string | undefined,
  order: readonly string[],
): MultiSelection {
  if (!order.length) return EMPTY_SELECTION;
  return { scope: scope ?? "", ids: [...order], anchor: order[0] };
}
