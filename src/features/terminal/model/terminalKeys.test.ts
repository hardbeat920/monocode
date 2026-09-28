import { describe, expect, it } from "vitest";
import { macTerminalShortcutData } from "./terminalKeys";

function key(
  key: string,
  modifiers: Partial<
    Pick<KeyboardEvent, "altKey" | "ctrlKey" | "metaKey" | "shiftKey">
  > = {},
) {
  return {
    key,
    altKey: false,
    ctrlKey: false,
    metaKey: false,
    shiftKey: false,
    ...modifiers,
  };
}

describe("mac terminal editing shortcuts", () => {
  it("sends shell word movement for Option+Arrow", () => {
    expect(macTerminalShortcutData(key("ArrowLeft", { altKey: true }))).toBe(
      "\x1bb",
    );
    expect(macTerminalShortcutData(key("ArrowRight", { altKey: true }))).toBe(
      "\x1bf",
    );
  });

  it("sends line movement and deletion for Command shortcuts", () => {
    expect(macTerminalShortcutData(key("ArrowLeft", { metaKey: true }))).toBe(
      "\x01",
    );
    expect(macTerminalShortcutData(key("ArrowRight", { metaKey: true }))).toBe(
      "\x05",
    );
    expect(macTerminalShortcutData(key("Backspace", { metaKey: true }))).toBe(
      "\x15",
    );
  });

  it("leaves other modifiers and keys to xterm and app shortcuts", () => {
    expect(
      macTerminalShortcutData(
        key("ArrowLeft", { metaKey: true, altKey: true }),
      ),
    ).toBeNull();
    expect(
      macTerminalShortcutData(
        key("ArrowLeft", { altKey: true, shiftKey: true }),
      ),
    ).toBeNull();
    expect(
      macTerminalShortcutData(key("ArrowLeft", { ctrlKey: true })),
    ).toBeNull();
    expect(
      macTerminalShortcutData(key("Backspace", { altKey: true })),
    ).toBeNull();
    expect(
      macTerminalShortcutData(key("Delete", { metaKey: true })),
    ).toBeNull();
  });
});
