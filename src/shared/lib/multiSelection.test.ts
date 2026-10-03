import { describe, expect, it } from "vitest";
import {
  EMPTY_SELECTION,
  applySelect,
  isSelected,
  pruneSelection,
  selectAll,
  selectionMode,
  selectionTargets,
  type MultiSelection,
} from "./multiSelection";

const order = ["a", "b", "c", "d", "e"];

function sel(
  ids: string[],
  anchor: string | null = ids[0] ?? null,
  scope: string | null = "",
): MultiSelection {
  return { scope, ids, anchor };
}

describe("selectionMode", () => {
  const event = { shiftKey: false, metaKey: false, ctrlKey: false };

  it("maps shift to range on every platform", () => {
    expect(selectionMode({ ...event, shiftKey: true }, true)).toBe("range");
    expect(selectionMode({ ...event, shiftKey: true }, false)).toBe("range");
    expect(
      selectionMode({ ...event, shiftKey: true, metaKey: true }, true),
    ).toBe("range");
  });

  it("toggles with Cmd on macOS, not Ctrl", () => {
    expect(selectionMode({ ...event, metaKey: true }, true)).toBe("toggle");
    expect(selectionMode({ ...event, ctrlKey: true }, true)).toBe("single");
  });

  it("toggles with Ctrl elsewhere, not Meta", () => {
    expect(selectionMode({ ...event, ctrlKey: true }, false)).toBe("toggle");
    expect(selectionMode({ ...event, metaKey: true }, false)).toBe("single");
  });

  it("defaults to single", () => {
    expect(selectionMode(event, true)).toBe("single");
    expect(selectionMode(event, false)).toBe("single");
  });
});

describe("applySelect", () => {
  it("single replaces the selection", () => {
    expect(
      applySelect(sel(["a", "b"]), { id: "c", mode: "single", order }),
    ).toEqual(sel(["c"], "c"));
  });

  it("single uses the click scope", () => {
    expect(
      applySelect(sel(["a"], "a", "staged"), {
        id: "b",
        scope: "unstaged",
        mode: "single",
        order,
      }),
    ).toEqual(sel(["b"], "b", "unstaged"));
  });

  it("toggle adds in insertion order and moves the anchor", () => {
    const next = applySelect(sel(["c"]), { id: "a", mode: "toggle", order });
    expect(next).toEqual(sel(["c", "a"], "a"));
  });

  it("toggle removes and still moves the anchor", () => {
    expect(
      applySelect(sel(["a", "b"], "a"), { id: "b", mode: "toggle", order }),
    ).toEqual(sel(["a"], "b"));
  });

  it("toggling the last id off keeps the anchor for a following range", () => {
    const off = applySelect(sel(["b"]), { id: "b", mode: "toggle", order });
    expect(off).toEqual(sel([], "b"));
    expect(applySelect(off, { id: "d", mode: "range", order })).toEqual(
      sel(["b", "c", "d"], "b"),
    );
  });

  it("toggle in another scope starts fresh", () => {
    expect(
      applySelect(sel(["a", "b"], "a", "staged"), {
        id: "c",
        scope: "unstaged",
        mode: "toggle",
        order,
      }),
    ).toEqual(sel(["c"], "c", "unstaged"));
  });

  it("range selects from the anchor in order order", () => {
    expect(applySelect(sel(["b"]), { id: "d", mode: "range", order })).toEqual(
      sel(["b", "c", "d"], "b"),
    );
  });

  it("range works backwards", () => {
    expect(applySelect(sel(["d"]), { id: "b", mode: "range", order })).toEqual(
      sel(["b", "c", "d"], "d"),
    );
  });

  it("successive ranges pivot around the same anchor", () => {
    const first = applySelect(sel(["c"]), { id: "e", mode: "range", order });
    expect(first).toEqual(sel(["c", "d", "e"], "c"));
    const second = applySelect(first, { id: "a", mode: "range", order });
    expect(second).toEqual(sel(["a", "b", "c"], "c"));
  });

  it("range replaces a toggled selection", () => {
    expect(
      applySelect(sel(["a", "e", "c"], "c"), {
        id: "d",
        mode: "range",
        order,
      }),
    ).toEqual(sel(["c", "d"], "c"));
  });

  it("range falls back to fallbackAnchor without an anchor", () => {
    expect(
      applySelect(EMPTY_SELECTION, {
        id: "d",
        mode: "range",
        order,
        fallbackAnchor: "b",
      }),
    ).toEqual(sel(["b", "c", "d"], "b"));
  });

  it("range ignores an anchor from another scope", () => {
    expect(
      applySelect(sel(["a"], "a", "staged"), {
        id: "d",
        scope: "unstaged",
        mode: "range",
        order,
        fallbackAnchor: "c",
      }),
    ).toEqual(sel(["c", "d"], "c", "unstaged"));
  });

  it("range ignores an anchor no longer in order", () => {
    expect(
      applySelect(sel(["x"], "x"), {
        id: "c",
        mode: "range",
        order,
        fallbackAnchor: "a",
      }),
    ).toEqual(sel(["a", "b", "c"], "a"));
  });

  it("range without a usable anchor behaves like single", () => {
    expect(
      applySelect(EMPTY_SELECTION, {
        id: "c",
        mode: "range",
        order,
        fallbackAnchor: "missing",
      }),
    ).toEqual(sel(["c"], "c"));
    expect(
      applySelect(EMPTY_SELECTION, {
        id: "c",
        mode: "range",
        order,
        fallbackAnchor: null,
      }),
    ).toEqual(sel(["c"], "c"));
  });

  it("range to an id missing from order behaves like single", () => {
    expect(applySelect(sel(["a"]), { id: "zz", mode: "range", order })).toEqual(
      sel(["zz"], "zz"),
    );
  });

  it("does not mutate inputs", () => {
    const selection = sel(["a", "b"], "a");
    const frozenOrder = Object.freeze([...order]);
    Object.freeze(selection.ids);
    Object.freeze(selection);
    applySelect(selection, { id: "c", mode: "toggle", order: frozenOrder });
    applySelect(selection, { id: "a", mode: "toggle", order: frozenOrder });
    applySelect(selection, { id: "e", mode: "range", order: frozenOrder });
    expect(selection).toEqual(sel(["a", "b"], "a"));
  });
});

describe("pruneSelection", () => {
  it("returns the same object when nothing is dropped", () => {
    const selection = sel(["a", "b"], "a");
    expect(pruneSelection(selection, () => true)).toBe(selection);
    expect(pruneSelection(EMPTY_SELECTION, () => false)).toBe(EMPTY_SELECTION);
  });

  it("drops ids and the anchor that fail keep", () => {
    const selection = sel(["a", "b", "c"], "b", "staged");
    expect(pruneSelection(selection, (id) => id !== "b")).toEqual(
      sel(["a", "c"], null, "staged"),
    );
    expect(selection.ids).toEqual(["a", "b", "c"]);
  });

  it("drops a stale anchor even when every id is kept", () => {
    expect(pruneSelection(sel(["a"], "x"), (id) => id !== "x")).toEqual(
      sel(["a"], null),
    );
  });

  it("keeps an anchor-only selection", () => {
    expect(pruneSelection(sel(["a"], "b"), (id) => id === "b")).toEqual(
      sel([], "b"),
    );
  });

  it("returns EMPTY_SELECTION when nothing remains", () => {
    expect(pruneSelection(sel(["a", "b"], "a"), () => false)).toBe(
      EMPTY_SELECTION,
    );
  });
});

describe("isSelected", () => {
  it("checks the id within the scope", () => {
    const selection = sel(["a"], "a", "staged");
    expect(isSelected(selection, "a", "staged")).toBe(true);
    expect(isSelected(selection, "a", "unstaged")).toBe(false);
    expect(isSelected(selection, "b", "staged")).toBe(false);
  });

  it("defaults to the empty scope", () => {
    expect(isSelected(sel(["a"]), "a")).toBe(true);
    expect(isSelected(EMPTY_SELECTION, "a")).toBe(false);
  });
});

describe("selectionTargets", () => {
  const selection = sel(["a", "c"], "a", "staged");

  it("returns the whole selection when the id is part of it", () => {
    const targets = selectionTargets(selection, "c", "staged");
    expect(targets).toEqual(["a", "c"]);
    expect(targets).not.toBe(selection.ids);
  });

  it("returns just the id when it is not selected", () => {
    expect(selectionTargets(selection, "b", "staged")).toEqual(["b"]);
  });

  it("returns just the id from another scope", () => {
    expect(selectionTargets(selection, "a", "unstaged")).toEqual(["a"]);
  });
});

describe("selectAll", () => {
  it("selects every id anchored on the first", () => {
    expect(selectAll("staged", order)).toEqual(sel(order, "a", "staged"));
    expect(selectAll(undefined, ["x", "y"])).toEqual(sel(["x", "y"], "x"));
  });

  it("returns EMPTY_SELECTION for an empty order", () => {
    expect(selectAll("staged", [])).toBe(EMPTY_SELECTION);
  });
});
