import { describe, expect, it } from "vitest";
import {
  isQuickComposerShortcut,
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
    expect(isQuickComposerShortcut("Shift+Space")).toBe(false);
    expect(isQuickComposerShortcut("Command+Command+Space")).toBe(false);
    expect(isQuickComposerShortcut("Option+KeyK")).toBe(false);
    expect(isQuickComposerShortcut("Command+KeyK")).toBe(true);
    expect(isQuickComposerShortcut("Control+KeyK")).toBe(true);
  });
});
