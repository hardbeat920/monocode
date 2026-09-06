// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { handleArchiveShortcut } from "./archiveShortcut";

afterEach(() => {
  document.body.replaceChildren();
});

function fixture() {
  document.body.innerHTML = "<div data-composer><textarea></textarea></div>";
  const composer = document.querySelector("textarea")!;
  composer.focus();
  const context = {
    activeTabId: "tab",
    tabs: [{ id: "tab", focusedId: "session", diffFocused: false }],
    sessions: [{ id: "session" }, { id: "other" }],
    projectTerminalFocused: false,
    surfaceOpen: false,
  };
  const archive = vi.fn();
  return {
    context,
    archive,
    composer,
    press(target: Element = composer, init: KeyboardEventInit = {}) {
      const received = vi.fn();
      const onKey = (event: KeyboardEvent) => {
        handleArchiveShortcut(event, context, archive);
      };
      window.addEventListener("keydown", onKey, true);
      target.addEventListener("keydown", received);
      const event = new KeyboardEvent("keydown", {
        key: "A",
        metaKey: true,
        shiftKey: true,
        bubbles: true,
        cancelable: true,
        ...init,
      });
      try {
        target.dispatchEvent(event);
      } finally {
        window.removeEventListener("keydown", onKey, true);
        target.removeEventListener("keydown", received);
      }
      return { event, received };
    },
  };
}

function expectUntouched(f: ReturnType<typeof fixture>, target?: Element) {
  const { event, received } = f.press(target);
  expect(event.defaultPrevented).toBe(false);
  expect(received).toHaveBeenCalledOnce();
  expect(f.archive).not.toHaveBeenCalled();
}

describe("archive shortcut routing", () => {
  it.each([{ metaKey: true }, { metaKey: false, ctrlKey: true }])(
    "consumes the shortcut only when archiving the focused conversation (%j)",
    (modifiers) => {
      const f = fixture();
      f.context.tabs[0].focusedId = "other";
      const { event, received } = f.press(f.composer, modifiers);
      expect(event.defaultPrevented).toBe(true);
      expect(received).not.toHaveBeenCalled();
      expect(f.archive).toHaveBeenCalledExactlyOnceWith("other");
    },
  );

  it.each(["editor", "terminal"])(
    "leaves a focused %s pane's key untouched",
    (pane) => {
      const f = fixture();
      f.context.tabs[0].focusedId = pane;
      expectUntouched(f);
    },
  );

  it.each(["diff", "project terminal", "app surface", "missing tab"])(
    "leaves the key untouched with %s focus",
    (focus) => {
      const f = fixture();
      if (focus === "diff") f.context.tabs[0].diffFocused = true;
      if (focus === "project terminal") f.context.projectTerminalFocused = true;
      if (focus === "app surface") f.context.surfaceOpen = true;
      if (focus === "missing tab") f.context.activeTabId = "missing";
      expectUntouched(f);
    },
  );

  it.each(["cm-editor", "monocode-terminal"])(
    "respects a %s event target before workspace focus catches up",
    (className) => {
      const f = fixture();
      const pane = document.createElement("div");
      pane.className = className;
      pane.innerHTML = "<textarea></textarea>";
      document.body.append(pane);
      expectUntouched(f, pane.firstElementChild!);
    },
  );

  it("leaves ordinary text inputs outside the composer untouched", () => {
    const f = fixture();
    const input = document.createElement("input");
    document.body.append(input);
    expectUntouched(f, input);
  });

  it.each([
    '<div role="menu"><input aria-label="Group name"></div>',
    '<div data-popover-side="bottom"><input></div>',
    '<div data-popover-side="top" role="listbox"></div>',
    '<div data-popover-side="top" role="toolbar"></div>',
    '<div role="dialog"><textarea></textarea></div>',
    '<div role="alertdialog"></div>',
    "<div data-skill-picker></div>",
    "<div data-mention-picker></div>",
  ])(
    "blocks open overlays even while focus stays in the composer: %s",
    (markup) => {
      const f = fixture();
      document.body.insertAdjacentHTML("beforeend", markup);
      expect(document.activeElement).toBe(f.composer);
      expectUntouched(f);
    },
  );

  it("leaves TabGroupMenu's rename input shortcut untouched", () => {
    const f = fixture();
    document.body.insertAdjacentHTML(
      "beforeend",
      '<div role="menu"><input aria-label="Group name"></div>',
    );
    const input = document.querySelector("input")!;
    input.focus();
    expectUntouched(f, input);
  });

  it.each([
    "hidden",
    "inert",
    'aria-hidden="true"',
    'style="display: none"',
    'style="visibility: hidden"',
  ])("ignores overlays in hidden surfaces (%s)", (attribute) => {
    const f = fixture();
    document.body.insertAdjacentHTML(
      "beforeend",
      `<div ${attribute}><div data-skill-picker></div></div>`,
    );
    f.press();
    expect(f.archive).toHaveBeenCalledExactlyOnceWith("session");
  });

  it("resumes archiving after the popover closes", () => {
    const f = fixture();
    document.body.insertAdjacentHTML(
      "beforeend",
      '<div data-popover-side="bottom"></div>',
    );
    expectUntouched(f);
    document.querySelector("[data-popover-side]")!.remove();
    f.press();
    expect(f.archive).toHaveBeenCalledExactlyOnceWith("session");
  });

  it("does not archive or stop propagation when another handler consumed the key", () => {
    const f = fixture();
    const consume = (event: KeyboardEvent) => event.preventDefault();
    window.addEventListener("keydown", consume, true);
    try {
      const { event, received } = f.press();
      expect(event.defaultPrevented).toBe(true);
      expect(received).toHaveBeenCalledOnce();
      expect(f.archive).not.toHaveBeenCalled();
    } finally {
      window.removeEventListener("keydown", consume, true);
    }
  });

  it.each([
    { repeat: true },
    { isComposing: true },
    { shiftKey: false },
    { altKey: true },
  ])("leaves ineligible key events untouched (%j)", (init) => {
    const f = fixture();
    const { event, received } = f.press(f.composer, init);
    expect(event.defaultPrevented).toBe(false);
    expect(received).toHaveBeenCalledOnce();
    expect(f.archive).not.toHaveBeenCalled();
  });
});
