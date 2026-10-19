// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  installNativeContextMenuGuard,
  shouldHideNativeContextMenu,
  type ContextMenuEventLike,
  type ContextMenuSelection,
} from "./nativeContextMenu";

function mount(html: string): HTMLElement {
  const container = document.createElement("div");
  container.innerHTML = html;
  document.body.append(container);
  return container;
}

function menuEvent(
  target: EventTarget | null,
  overrides: Partial<ContextMenuEventLike> = {},
): ContextMenuEventLike {
  return {
    target,
    defaultPrevented: false,
    clientX: 10,
    clientY: 10,
    shiftKey: false,
    ...overrides,
  };
}

/** A selection drawn as one rectangle, left 0 to 100 and top 0 to 20. */
function selectionAt(isCollapsed = false): ContextMenuSelection {
  const rect = { left: 0, right: 100, top: 0, bottom: 20 };
  return {
    isCollapsed,
    rangeCount: 1,
    getRangeAt: () => ({ getClientRects: () => [rect] }) as unknown as Range,
  };
}

afterEach(() => {
  document.body.innerHTML = "";
});

describe("shouldHideNativeContextMenu", () => {
  it("hides the page menu on plain areas", () => {
    const root = mount('<div class="transcript"><p>Hello</p></div>');
    expect(
      shouldHideNativeContextMenu(menuEvent(root.querySelector("p")), null),
    ).toBe(true);
    expect(
      shouldHideNativeContextMenu(
        menuEvent(root.querySelector("p")!.firstChild),
        null,
      ),
    ).toBe(true);
    expect(shouldHideNativeContextMenu(menuEvent(document), null)).toBe(true);
  });

  it("leaves events a custom menu already handled alone", () => {
    const root = mount("<div></div>");
    expect(
      shouldHideNativeContextMenu(
        menuEvent(root.firstChild, { defaultPrevented: true }),
        null,
      ),
    ).toBe(false);
  });

  it("keeps the text menu in editable targets", () => {
    const root = mount(`
      <textarea></textarea>
      <input type="text" />
      <input />
      <input type="search" />
      <div contenteditable="true"><span id="rich">text</span></div>
      <div contenteditable=""><span id="bare">text</span></div>
      <div class="cm-editor"><div class="cm-scroller"><div class="cm-content" contenteditable="true"><div class="cm-line">code</div></div><div class="cm-gutters"></div></div></div>
      <div class="monocode-terminal"><div class="xterm"><div class="xterm-screen"></div></div></div>
    `);
    for (const selector of [
      "textarea",
      'input[type="text"]',
      "input:not([type])",
      'input[type="search"]',
      "#rich",
      "#bare",
      ".cm-line",
      ".cm-gutters",
      ".xterm-screen",
    ]) {
      expect(
        shouldHideNativeContextMenu(
          menuEvent(root.querySelector(selector)),
          null,
        ),
        selector,
      ).toBe(false);
    }
  });

  it("hides the page menu on controls without editable text", () => {
    const root = mount(`
      <input type="checkbox" />
      <input type="range" />
      <button><svg></svg></button>
      <div contenteditable="false"><span id="locked">text</span></div>
    `);
    for (const selector of [
      'input[type="checkbox"]',
      'input[type="range"]',
      "svg",
      "#locked",
    ]) {
      expect(
        shouldHideNativeContextMenu(
          menuEvent(root.querySelector(selector)),
          null,
        ),
        selector,
      ).toBe(true);
    }
  });

  it("keeps the link and media menus", () => {
    const root = mount(`
      <a href="https://example.com"><span>link</span></a>
      <img alt="" />
      <video></video>
      <a id="anchor">no href</a>
    `);
    for (const selector of ["a span", "img", "video"]) {
      expect(
        shouldHideNativeContextMenu(
          menuEvent(root.querySelector(selector)),
          null,
        ),
        selector,
      ).toBe(false);
    }
    expect(
      shouldHideNativeContextMenu(
        menuEvent(root.querySelector("#anchor")),
        null,
      ),
    ).toBe(true);
  });

  it("keeps the text menu when the click lands on the selection", () => {
    const root = mount("<p>Hello</p>");
    const p = root.querySelector("p");
    expect(
      shouldHideNativeContextMenu(
        menuEvent(p, { clientX: 50, clientY: 10 }),
        selectionAt(),
      ),
    ).toBe(false);
    expect(
      shouldHideNativeContextMenu(
        menuEvent(p, { clientX: 150, clientY: 10 }),
        selectionAt(),
      ),
    ).toBe(true);
    expect(
      shouldHideNativeContextMenu(
        menuEvent(p, { clientX: 50, clientY: 10 }),
        selectionAt(true),
      ),
    ).toBe(true);
  });

  it("keeps Inspect Element behind Shift only when allowed", () => {
    const root = mount("<div></div>");
    const event = menuEvent(root.firstChild, { shiftKey: true });
    expect(
      shouldHideNativeContextMenu(event, null, { allowInspect: true }),
    ).toBe(false);
    expect(shouldHideNativeContextMenu(event, null)).toBe(true);
    expect(
      shouldHideNativeContextMenu(menuEvent(root.firstChild), null, {
        allowInspect: true,
      }),
    ).toBe(true);
  });
});

describe("installNativeContextMenuGuard", () => {
  function contextMenu(target: Element, init: MouseEventInit = {}) {
    const event = new MouseEvent("contextmenu", {
      bubbles: true,
      cancelable: true,
      ...init,
    });
    target.dispatchEvent(event);
    return event;
  }

  it("cancels the page menu and keeps the text menu", () => {
    const root = mount("<div><span>plain</span></div><textarea></textarea>");
    const uninstall = installNativeContextMenuGuard(window, false);
    try {
      expect(contextMenu(root.querySelector("span")!).defaultPrevented).toBe(
        true,
      );
      expect(
        contextMenu(root.querySelector("textarea")!).defaultPrevented,
      ).toBe(false);
    } finally {
      uninstall();
    }
    expect(contextMenu(root.querySelector("span")!).defaultPrevented).toBe(
      false,
    );
  });

  it("runs after React handlers, which still see an uncancelled event", () => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    const container = mount("");
    const root = createRoot(container);
    const seen: boolean[] = [];
    const uninstall = installNativeContextMenuGuard(window, false);
    try {
      act(() =>
        root.render(
          createElement(
            "div",
            {
              id: "custom",
              onContextMenu: (event: { defaultPrevented: boolean }) => {
                seen.push(event.defaultPrevented);
              },
            },
            "item",
          ),
        ),
      );
      const event = contextMenu(container.querySelector("#custom")!);
      expect(seen).toEqual([false]);
      expect(event.defaultPrevented).toBe(true);
    } finally {
      uninstall();
      act(() => root.unmount());
      vi.unstubAllGlobals();
    }
  });
});
