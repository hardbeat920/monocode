import { describe, expect, it, vi } from "vitest";

vi.mock("../../../platform/tauri/platform", () => ({ IS_MAC: true }));

import {
  isGlobalShortcut,
  isShortcut,
  quickComposerShortcutLabel,
  quickComposerShortcutPreview,
  shortcutFromKeyEvent,
} from "./quickComposerShortcut";

describe("quick composer shortcut", () => {
  it("records physical keys and displays the chosen combination", () => {
    const shortcut = shortcutFromKeyEvent({
      code: "KeyK",
      metaKey: true,
      ctrlKey: false,
      altKey: true,
      shiftKey: false,
    });
    expect(shortcut).toBe("Command+Option+KeyK");
    expect(quickComposerShortcutLabel(shortcut!)).toBe("⌘⌥K");
    expect(
      quickComposerShortcutPreview({
        metaKey: true,
        ctrlKey: false,
        altKey: false,
        shiftKey: false,
      }),
    ).toBe("⌘");
    expect(
      quickComposerShortcutPreview(
        {
          metaKey: true,
          ctrlKey: false,
          altKey: false,
          shiftKey: false,
        },
        "KeyK",
      ),
    ).toBe("⌘K");
    expect(
      shortcutFromKeyEvent({
        code: "KeyK",
        metaKey: true,
        ctrlKey: false,
        altKey: false,
        shiftKey: false,
      }),
    ).toBe("Command+KeyK");
    expect(
      shortcutFromKeyEvent({
        code: "Digit2",
        metaKey: false,
        ctrlKey: true,
        altKey: false,
        shiftKey: true,
      }),
    ).toBe("Control+Shift+Digit2");
  });

  it("rejects plain typing, modifier keys, and unsupported codes", () => {
    expect(
      shortcutFromKeyEvent({
        code: "Space",
        metaKey: false,
        ctrlKey: false,
        altKey: false,
        shiftKey: true,
      }),
    ).toBeNull();
    expect(
      shortcutFromKeyEvent({
        code: "MetaLeft",
        metaKey: true,
        ctrlKey: false,
        altKey: false,
        shiftKey: false,
      }),
    ).toBeNull();
    expect(isShortcut("Shift+Space")).toBe(false);
    expect(isShortcut("Command+Command+Space")).toBe(false);
    expect(isShortcut("Command+KeyK")).toBe(true);
    expect(isShortcut("Control+KeyK")).toBe(true);
  });

  it("records Option chords but keeps them out of OS-global hotkeys", () => {
    const alt = shortcutFromKeyEvent({
      code: "KeyK",
      metaKey: false,
      ctrlKey: false,
      altKey: true,
      shiftKey: false,
    });
    expect(alt).toBe("Option+KeyK");
    const altShift = shortcutFromKeyEvent({
      code: "KeyK",
      metaKey: false,
      ctrlKey: false,
      altKey: true,
      shiftKey: true,
    });
    expect(altShift).toBe("Option+Shift+KeyK");
    expect(
      shortcutFromKeyEvent({
        code: "KeyK",
        metaKey: false,
        ctrlKey: false,
        altKey: false,
        shiftKey: true,
      }),
    ).toBeNull();

    expect(isShortcut("Option+KeyK")).toBe(true);
    expect(isGlobalShortcut("Option+KeyK")).toBe(false);
    expect(isGlobalShortcut("Control+Option+KeyK")).toBe(true);
  });
});
