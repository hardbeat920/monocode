import { beforeEach, describe, expect, it } from "vitest";
import { resolveAppShortcut } from "./appShortcuts";
import { saveKeybindingOverride } from "./settings";

function key(partial: Partial<Parameters<typeof resolveAppShortcut>[0]>) {
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

describe("resolveAppShortcut", () => {
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

  it("resolves the default app chords", () => {
    expect(resolveAppShortcut(key({ key: "k", metaKey: true }))).toBe(
      "App: Search",
    );
    expect(
      resolveAppShortcut(key({ key: "P", metaKey: true, shiftKey: true })),
    ).toBe("App: Command Palette");
    expect(resolveAppShortcut(key({ key: ",", ctrlKey: true }))).toBe(
      "App: Settings",
    );
    expect(
      resolveAppShortcut(key({ key: "b", metaKey: true, shiftKey: true })),
    ).toBe("App: Toggle Session Sidebar");
  });

  it("ignores a chord while an IME is composing", () => {
    // The maintainer's blocker: these used to fire mid-composition.
    for (const chord of [
      { key: "k", metaKey: true },
      { key: "P", metaKey: true, shiftKey: true },
      { key: ",", ctrlKey: true },
      { key: "b", metaKey: true, shiftKey: true },
      { key: "f", metaKey: true, shiftKey: true },
    ]) {
      expect(
        resolveAppShortcut(key({ ...chord, isComposing: true })),
      ).toBeNull();
    }
  });

  it("ignores a rebound chord while composing", () => {
    saveKeybindingOverride("App: Search", { shortcut: "Command+Shift+KeyM" });
    expect(
      resolveAppShortcut(
        key({
          key: "M",
          code: "KeyM",
          metaKey: true,
          shiftKey: true,
          isComposing: true,
        }),
      ),
    ).toBeNull();
  });

  it("fires a rebound chord and drops the default it replaced", () => {
    saveKeybindingOverride("App: Search", { shortcut: "Command+Shift+KeyM" });
    expect(
      resolveAppShortcut(
        key({ key: "M", code: "KeyM", metaKey: true, shiftKey: true }),
      ),
    ).toBe("App: Search");
    expect(resolveAppShortcut(key({ key: "k", metaKey: true }))).toBeNull();
  });

  it("requires a primary modifier and refuses Alt", () => {
    expect(resolveAppShortcut(key({ key: "k" }))).toBeNull();
    expect(
      resolveAppShortcut(key({ key: "k", metaKey: true, altKey: true })),
    ).toBeNull();
  });

  it("honours a disabled command", () => {
    saveKeybindingOverride("App: Search", { disabled: true });
    expect(resolveAppShortcut(key({ key: "k", metaKey: true }))).toBeNull();
  });
});
