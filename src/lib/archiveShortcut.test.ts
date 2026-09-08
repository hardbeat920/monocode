import { afterEach, describe, expect, it, vi } from "vitest";
import { archiveFocusedSession } from "./archiveShortcut";

// Only the DOM operations used by the handler are needed in the Node suite.
class ElementStub {
  constructor(readonly ancestors: string[] = []) {}
  closest(selector: string) {
    return selector.split(", ").some((part) => this.ancestors.includes(part))
      ? this
      : null;
  }
  getClientRects = vi.fn(() => [{}]);
}

afterEach(() => vi.unstubAllGlobals());

function fixture() {
  const overlays: { selector: string; element: ElementStub }[] = [];
  vi.stubGlobal("Element", ElementStub);
  const documentStub = {
    activeElement: null as ElementStub | null,
    hasFocus: vi.fn(() => true),
    querySelectorAll: (selector: string) =>
      overlays
        .filter((overlay) => selector.split(", ").includes(overlay.selector))
        .map((overlay) => overlay.element),
  };
  vi.stubGlobal("document", documentStub);
  const style = vi.fn(() => ({ visibility: "visible" }));
  vi.stubGlobal("getComputedStyle", style);
  const context = {
    activeTabId: "tab",
    tabs: [{ id: "tab", focusedId: "session", diffFocused: false }],
    sessions: [{ id: "session" }, { id: "other" }],
    projectTerminalFocused: false,
    surfaceOpen: false,
  };
  const event = {
    defaultPrevented: false,
    target: new ElementStub(["textarea", "[data-composer]"]),
    preventDefault: vi.fn(),
    stopPropagation: vi.fn(),
  };
  const archive = vi.fn();
  return {
    context,
    event,
    archive,
    overlays,
    style,
    document: documentStub,
    runMenu: () => archiveFocusedSession(undefined, context, archive),
    run: () =>
      archiveFocusedSession(
        event as unknown as KeyboardEvent,
        context,
        archive,
      ),
  };
}

function expectUntouched(f: ReturnType<typeof fixture>) {
  f.run();
  expect(f.event.preventDefault).not.toHaveBeenCalled();
  expect(f.event.stopPropagation).not.toHaveBeenCalled();
  expect(f.archive).not.toHaveBeenCalled();
}

describe("archive shortcut routing", () => {
  it("archives from a native close command in the focused window", () => {
    const f = fixture();
    f.runMenu();
    expect(f.archive).toHaveBeenCalledExactlyOnceWith("session");
  });

  it("does not archive in another window receiving the native menu event", () => {
    const f = fixture();
    f.document.hasFocus.mockReturnValue(false);
    f.runMenu();
    expect(f.archive).not.toHaveBeenCalled();
  });

  it("checks DOM focus for native commands without a keyboard event", () => {
    const f = fixture();
    f.document.activeElement = new ElementStub([".cm-editor"]);
    f.runMenu();
    expect(f.archive).not.toHaveBeenCalled();
  });

  it("consumes the event before archiving exactly the focused session", () => {
    const f = fixture();
    f.context.tabs[0].focusedId = "other";
    f.archive.mockImplementation(() => {
      expect(f.event.preventDefault).toHaveBeenCalledOnce();
      expect(f.event.stopPropagation).toHaveBeenCalledOnce();
    });
    f.run();
    expect(f.archive).toHaveBeenCalledExactlyOnceWith("other");
  });

  it.each(["editor", "terminal"])(
    "preserves the key in a focused %s pane",
    (pane) => {
      const f = fixture();
      f.context.tabs[0].focusedId = pane;
      expectUntouched(f);
    },
  );

  it.each(["diff", "dock", "surface", "missing tab", "already handled"])(
    "preserves the key when blocked by %s",
    (reason) => {
      const f = fixture();
      if (reason === "diff") f.context.tabs[0].diffFocused = true;
      if (reason === "dock") f.context.projectTerminalFocused = true;
      if (reason === "surface") f.context.surfaceOpen = true;
      if (reason === "missing tab") f.context.activeTabId = "missing";
      if (reason === "already handled") f.event.defaultPrevented = true;
      expectUntouched(f);
    },
  );

  it.each([".cm-editor", ".monocode-terminal", "input"])(
    "respects %s DOM focus even before workspace focus updates",
    (ancestor) => {
      const f = fixture();
      f.event.target = new ElementStub([ancestor]);
      expectUntouched(f);
    },
  );

  it.each([
    '[role="menu"]',
    '[role="dialog"]',
    '[role="alertdialog"]',
    "[data-popover-side]",
    "[data-skill-picker]",
    "[data-mention-picker]",
  ])(
    "blocks an open %s even when focus remains in the composer",
    (selector) => {
      const f = fixture();
      f.overlays.push({ selector, element: new ElementStub() });
      expectUntouched(f);
      f.overlays.pop();
      f.run();
      expect(f.archive).toHaveBeenCalledExactlyOnceWith("session");
    },
  );

  it("leaves a TabGroupMenu rename input event untouched", () => {
    const f = fixture();
    f.event.target = new ElementStub(['[role="menu"]', "input"]);
    f.overlays.push({ selector: '[role="menu"]', element: new ElementStub() });
    expectUntouched(f);
  });

  it.each([
    "no layout",
    "hidden visibility",
    "[hidden]",
    "[inert]",
    '[aria-hidden="true"]',
  ])("ignores an inactive overlay (%s)", (hidden) => {
    const f = fixture();
    const element = new ElementStub([hidden]);
    if (hidden === "no layout") element.getClientRects.mockReturnValue([]);
    if (hidden === "hidden visibility")
      f.style.mockReturnValue({ visibility: "hidden" });
    f.overlays.push({ selector: "[data-skill-picker]", element });
    f.run();
    expect(f.archive).toHaveBeenCalledExactlyOnceWith("session");
  });
});
