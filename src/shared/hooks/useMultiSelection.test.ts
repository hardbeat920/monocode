// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const platform = vi.hoisted(() => ({ mac: true }));

vi.mock("../../platform/tauri/platform", () => ({
  get IS_MAC() {
    return platform.mac;
  },
}));

import { EMPTY_SELECTION } from "../lib/multiSelection";
import {
  useMultiSelection,
  type MultiSelectionOptions,
} from "./useMultiSelection";

type Hook = ReturnType<typeof useMultiSelection>;

const ORDERS: Record<string, string[]> = {
  "": ["a", "b", "c", "d", "e"],
  staged: ["s1", "s2", "s3"],
};

let container: HTMLDivElement;
let root: Root;
let hook: Hook;
let renderedA: boolean;

function Harness({ options }: { options: MultiSelectionOptions }) {
  hook = useMultiSelection(options);
  renderedA = hook.isSelected("a");
  return null;
}

function render(options: Partial<MultiSelectionOptions> = {}) {
  act(() =>
    root.render(
      createElement(Harness, {
        options: { visibleOrder: (scope) => ORDERS[scope] ?? [], ...options },
      }),
    ),
  );
}

const plain = { shiftKey: false, metaKey: false, ctrlKey: false };
const cmd = { ...plain, metaKey: true };
const ctrl = { ...plain, ctrlKey: true };
const shift = { ...plain, shiftKey: true };

function key(init: KeyboardEventInit, target: EventTarget = container) {
  const event = new KeyboardEvent("keydown", {
    bubbles: true,
    cancelable: true,
    ...init,
  });
  // Dispatched so the event has a real target, as a container listener would see it.
  let acted = false;
  target.addEventListener(
    "keydown",
    (e) => {
      act(() => {
        acted = hook.onKeyDown(e as KeyboardEvent);
      });
    },
    { once: true },
  );
  target.dispatchEvent(event);
  return { event, acted };
}

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  platform.mac = true;
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

describe("useMultiSelection clicks", () => {
  it("applies plain, toggle and range clicks and returns the mode", () => {
    render();
    let mode = "";
    act(() => {
      mode = hook.onRowClick(plain, "b");
    });
    expect(mode).toBe("single");
    expect(hook.selection.ids).toEqual(["b"]);

    act(() => {
      mode = hook.onRowClick(cmd, "d");
    });
    expect(mode).toBe("toggle");
    expect(hook.selection.ids).toEqual(["b", "d"]);

    act(() => {
      mode = hook.onRowClick(shift, "b");
    });
    expect(mode).toBe("range");
    expect(hook.selection.ids).toEqual(["b", "c", "d"]);
    expect(hook.selection.scope).toBe("");
  });

  it("toggles with Ctrl off macOS", () => {
    platform.mac = false;
    render();
    act(() => {
      hook.onRowClick(plain, "a");
    });
    let mode = "";
    act(() => {
      mode = hook.onRowClick(ctrl, "c");
    });
    expect(mode).toBe("toggle");
    expect(hook.selection.ids).toEqual(["a", "c"]);
  });

  it("composes several calls in one tick", () => {
    render();
    act(() => {
      hook.select("a", undefined, "single");
      hook.select("c", undefined, "toggle");
      hook.select("e", undefined, "toggle");
    });
    expect(hook.selection.ids).toEqual(["a", "c", "e"]);
  });

  it("starts a first Shift-click range from the fallback anchor", () => {
    const fallbackAnchor = vi.fn((scope: string) =>
      scope === "staged" ? "s1" : null,
    );
    render({ fallbackAnchor });
    act(() => {
      hook.onRowClick(shift, "s3", "staged");
    });
    expect(fallbackAnchor).toHaveBeenCalledWith("staged");
    expect(hook.selection).toEqual({
      scope: "staged",
      ids: ["s1", "s2", "s3"],
      anchor: "s1",
    });
  });

  it("reads the latest visibleOrder closure without memoizing", () => {
    render({ visibleOrder: () => ["a", "b", "c"] });
    const first = hook.onRowClick;
    act(() => {
      hook.onRowClick(plain, "a");
    });
    render({ visibleOrder: () => ["a", "x", "c"] });
    act(() => {
      hook.onRowClick(shift, "c");
    });
    expect(hook.onRowClick).toBe(first);
    expect(hook.selection.ids).toEqual(["a", "x", "c"]);
  });

  it("keeps callback identities across re-renders, except isSelected", () => {
    render();
    const { selection: _s, isSelected: _i, ...before } = hook;
    act(() => {
      hook.onRowClick(plain, "a");
    });
    render();
    const { selection: _t, isSelected: _j, ...after } = hook;
    for (const name of Object.keys(before) as (keyof typeof before)[]) {
      expect(after[name], name).toBe(before[name]);
    }
  });
});

describe("useMultiSelection queries and bulk updates", () => {
  it("answers isSelected and targetsFor from the latest selection", () => {
    render();
    act(() => {
      hook.onRowClick(plain, "a");
      hook.onRowClick(cmd, "b");
    });
    expect(hook.isSelected("a")).toBe(true);
    expect(hook.isSelected("a", "staged")).toBe(false);
    expect(hook.isSelected("c")).toBe(false);
    expect(hook.targetsFor("b")).toEqual(["a", "b"]);
    expect(hook.targetsFor("c")).toEqual(["c"]);
    expect(hook.targetsFor("a", "staged")).toEqual(["a"]);
  });

  it("selects all of a scope and clears", () => {
    render();
    act(() => hook.selectAll("staged"));
    expect(hook.selection).toEqual({
      scope: "staged",
      ids: ["s1", "s2", "s3"],
      anchor: "s1",
    });
    act(() => hook.clear());
    expect(hook.selection).toBe(EMPTY_SELECTION);
  });

  it("answers isSelected from the selection being rendered", () => {
    render();
    expect(renderedA).toBe(false);
    act(() => {
      hook.onRowClick(plain, "a");
    });
    expect(renderedA).toBe(true);
    act(() => {
      hook.onRowClick(cmd, "a");
    });
    expect(renderedA).toBe(false);
  });

  it("keeps the same state when clearing an empty selection", () => {
    render();
    act(() => hook.clear());
    expect(hook.selection).toBe(EMPTY_SELECTION);
  });

  it("prunes ids and keeps the same state when nothing changes", () => {
    render();
    act(() => hook.selectAll());
    act(() => hook.prune((id) => id !== "b" && id !== "d"));
    expect(hook.selection.ids).toEqual(["a", "c", "e"]);

    const before = hook.selection;
    act(() => hook.prune(() => true));
    expect(hook.selection).toBe(before);
  });
});

describe("useMultiSelection batched updates", () => {
  it("prunes a selection queued in the same tick", () => {
    render();
    act(() => {
      hook.select("a", undefined, "single");
      hook.select("b", undefined, "toggle");
      hook.prune((id) => id !== "b");
    });
    expect(hook.selection.ids).toEqual(["a"]);
  });
});

describe("useMultiSelection onKeyDown", () => {
  it("clears a non-empty selection on Escape", () => {
    render();
    act(() => hook.selectAll());
    const { event, acted } = key({ key: "Escape" });
    expect(acted).toBe(true);
    expect(event.defaultPrevented).toBe(true);
    expect(hook.selection).toBe(EMPTY_SELECTION);
  });

  it("leaves Escape alone when nothing is selected", () => {
    render();
    const { event, acted } = key({ key: "Escape" });
    expect(acted).toBe(false);
    expect(event.defaultPrevented).toBe(false);
  });

  it.each([
    { mac: true, init: { key: "a", metaKey: true }, acts: true },
    { mac: true, init: { key: "a", ctrlKey: true }, acts: false },
    { mac: false, init: { key: "a", ctrlKey: true }, acts: true },
    { mac: false, init: { key: "a", metaKey: true }, acts: false },
  ])("selects all on Mod+A (mac=$mac, $init)", ({ mac, init, acts }) => {
    platform.mac = mac;
    render();
    act(() => {
      hook.onRowClick(plain, "s2", "staged");
    });
    const { event, acted } = key(init);
    expect(acted).toBe(acts);
    expect(event.defaultPrevented).toBe(acts);
    expect(hook.selection.ids).toEqual(acts ? ["s1", "s2", "s3"] : ["s2"]);
  });

  it("selects all of an explicit scope, and does nothing without one", () => {
    render();
    expect(key({ key: "a", metaKey: true }).acted).toBe(false);
    let acted = false;
    act(() => {
      acted = hook.onKeyDown(
        new KeyboardEvent("keydown", { key: "a", metaKey: true }),
        "staged",
      );
    });
    expect(acted).toBe(true);
    expect(hook.selection.ids).toEqual(["s1", "s2", "s3"]);
  });

  it.each(["input", "textarea", "contenteditable"])(
    "ignores events from %s targets",
    (kind) => {
      render();
      act(() => hook.selectAll());
      const el =
        kind === "contenteditable"
          ? document.createElement("div")
          : document.createElement(kind);
      if (kind === "contenteditable") el.setAttribute("contenteditable", "");
      container.append(el);
      expect(key({ key: "Escape" }, el).acted).toBe(false);
      expect(key({ key: "a", metaKey: true }, el).acted).toBe(false);
      expect(hook.selection.ids).toHaveLength(5);
    },
  );

  it("ignores events already defaultPrevented", () => {
    render();
    act(() => hook.selectAll());
    const event = new KeyboardEvent("keydown", {
      key: "Escape",
      cancelable: true,
    });
    event.preventDefault();
    let acted = true;
    act(() => {
      acted = hook.onKeyDown(event);
    });
    expect(acted).toBe(false);
    expect(hook.selection.ids).toHaveLength(5);
  });
});
