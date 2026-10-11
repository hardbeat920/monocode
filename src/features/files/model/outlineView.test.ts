import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  OUTLINE_VIEW_DEFAULT,
  OUTLINE_WIDTH_MAX,
  OUTLINE_WIDTH_MIN,
  loadOutlineView,
  saveOutlineView,
} from "./outlineView";

describe("outlineView store", () => {
  let storage: Map<string, string>;

  beforeEach(() => {
    storage = new Map();
    vi.stubGlobal("localStorage", {
      getItem: (key: string) => storage.get(key) ?? null,
      setItem: (key: string, value: string) => {
        storage.set(key, value);
      },
    });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("defaults to a closed docked panel", () => {
    expect(loadOutlineView()).toEqual(OUTLINE_VIEW_DEFAULT);
  });

  it("round-trips a saved view", () => {
    saveOutlineView({
      open: true,
      width: 300,
      detached: true,
      position: { x: 40, y: 60 },
    });
    expect(loadOutlineView()).toEqual({
      open: true,
      width: 300,
      detached: true,
      position: { x: 40, y: 60 },
    });
  });

  it("clamps the width into range", () => {
    saveOutlineView({ ...OUTLINE_VIEW_DEFAULT, width: 10_000 });
    expect(loadOutlineView().width).toBe(OUTLINE_WIDTH_MAX);
    saveOutlineView({ ...OUTLINE_VIEW_DEFAULT, width: 1 });
    expect(loadOutlineView().width).toBe(OUTLINE_WIDTH_MIN);
  });

  it("ignores malformed storage and bad positions", () => {
    storage.set("monocode.outlineView", "{not json");
    expect(loadOutlineView()).toEqual(OUTLINE_VIEW_DEFAULT);

    storage.set(
      "monocode.outlineView",
      JSON.stringify({ open: true, position: { x: "nope", y: 1 } }),
    );
    const view = loadOutlineView();
    expect(view.open).toBe(true);
    expect(view.position).toBeNull();
  });

  it("returns a stable snapshot between saves", () => {
    expect(loadOutlineView()).toBe(loadOutlineView());
  });
});
