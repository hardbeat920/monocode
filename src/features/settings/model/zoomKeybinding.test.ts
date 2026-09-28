import { beforeEach, describe, expect, it } from "vitest";
import { resolveZoomKeybinding } from "./zoomKeybinding";
import { saveKeybindingOverride } from "./settings";

function key(partial: Partial<Parameters<typeof resolveZoomKeybinding>[0]>) {
  return {
    code: "",
    key: "",
    metaKey: false,
    ctrlKey: false,
    altKey: false,
    shiftKey: false,
    isComposing: false,
    ...partial,
  };
}

describe("resolveZoomKeybinding", () => {
  beforeEach(() => {
    const data = new Map<string, string>();
    Object.defineProperty(globalThis, "localStorage", {
      configurable: true,
      value: {
        getItem: (k: string) => data.get(k) ?? null,
        setItem: (k: string, v: string) => void data.set(k, v),
        removeItem: (k: string) => void data.delete(k),
      },
    });
  });

  it("keeps the browser-standard chords working", () => {
    expect(resolveZoomKeybinding(key({ key: "+", ctrlKey: true }))).toBe(
      "zoom-in",
    );
    expect(resolveZoomKeybinding(key({ key: "-", metaKey: true }))).toBe(
      "zoom-out",
    );
    expect(resolveZoomKeybinding(key({ key: "0", ctrlKey: true }))).toBe(
      "zoom-reset",
    );
  });

  it("fires an Option-only rebound that the default guard would swallow", () => {
    saveKeybindingOverride("View: Zoom In", { shortcut: "Option+Equal" });
    expect(
      resolveZoomKeybinding(key({ key: "=", code: "Equal", altKey: true })),
    ).toBe("zoom-in");
  });

  it("fires an Option-only reset rebound", () => {
    saveKeybindingOverride("View: Reset Zoom", {
      shortcut: "Option+Digit0",
    });
    expect(
      resolveZoomKeybinding(key({ key: "0", code: "Digit0", altKey: true })),
    ).toBe("zoom-reset");
  });

  it("stops the replaced default from firing once rebound", () => {
    saveKeybindingOverride("View: Zoom In", { shortcut: "Option+Equal" });
    expect(resolveZoomKeybinding(key({ key: "+", ctrlKey: true }))).toBeNull();
  });

  it("honours a disabled zoom binding and ignores composition", () => {
    saveKeybindingOverride("View: Zoom In", { disabled: true });
    expect(resolveZoomKeybinding(key({ key: "+", ctrlKey: true }))).toBeNull();
    expect(
      resolveZoomKeybinding(
        key({ key: "+", ctrlKey: true, isComposing: true }),
      ),
    ).toBeNull();
  });
});
