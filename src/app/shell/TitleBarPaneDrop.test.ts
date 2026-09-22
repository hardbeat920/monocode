// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  getExternalPaneDrop,
  setExternalTitleTabDrop,
  titleTabDropFromPoint,
} from "../../features/workspace/model/paneDrop";
import { TitleBar, type Tab } from "./TitleBar";

vi.mock("./WindowControls", () => ({ WindowControls: () => null }));

let container: HTMLDivElement;
let root: Root;

function tab(id: string): Tab {
  return {
    id,
    project: "project",
    title: id,
    more: [],
    sessionCount: 1,
    harnesses: [],
    busyHarnesses: [],
    files: [],
  };
}

function pointer(
  target: EventTarget,
  type: string,
  clientX: number,
  clientY: number,
) {
  act(() =>
    target.dispatchEvent(
      new PointerEvent(type, {
        bubbles: true,
        button: 0,
        pointerId: 1,
        clientX,
        clientY,
      }),
    ),
  );
}

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => window.dispatchEvent(new Event("blur")));
  act(() => setExternalTitleTabDrop(null));
  act(() => root.unmount());
  container.remove();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("title tab pane drops", () => {
  it("joins compact navigation and tabs in one title bar", () => {
    const onGoBack = vi.fn();
    const onGoForward = vi.fn();
    act(() =>
      root.render(
        createElement(TitleBar, {
          tabs: [tab("first"), tab("second")],
          activeId: "first",
          cwd: "/project",
          projectRailOpen: false,
          compactRail: true,
          canGoBack: true,
          canGoForward: true,
          onGoBack,
          onGoForward,
          onToggleSidebar: vi.fn(),
          onNew: vi.fn(),
          onSelect: vi.fn(),
          onClose: vi.fn(),
          onCloseMany: vi.fn(),
          onReorder: vi.fn(),
        }),
      ),
    );

    const nav = container.querySelector<HTMLElement>(
      "[data-compact-title-nav]",
    )!;
    expect(nav.closest("header")?.className).toContain("body-glass");
    expect(nav.className).toContain("pl-[70px]");
    expect(
      Array.from(nav.querySelectorAll("button"), (button) =>
        button.getAttribute("aria-label")?.replace(/ \(.+\)$/, ""),
      ),
    ).toEqual(["Back", "Forward"]);
    expect(nav.nextElementSibling?.querySelector("[data-title-tab-strip]")).not
      .toBeNull;
    expect(container.textContent).not.toContain("Development");

    act(() => nav.querySelector<HTMLButtonElement>("button")!.click());
    expect(onGoBack).toHaveBeenCalledOnce();
  });

  it("keeps the current tab visible and places the dragged tab on a pane edge", () => {
    const onSelect = vi.fn();
    const onPlaceOnPane = vi.fn();
    act(() =>
      root.render(
        createElement(TitleBar, {
          tabs: [tab("first"), tab("second")],
          activeId: "first",
          cwd: "/project",
          onToggleSidebar: vi.fn(),
          onNew: vi.fn(),
          onSelect,
          onClose: vi.fn(),
          onCloseMany: vi.fn(),
          onReorder: vi.fn(),
          onPlaceOnPane,
        }),
      ),
    );

    const titleButtons = ["first", "second"].map((id) =>
      container.querySelector<HTMLButtonElement>(
        `button[aria-label="project · ${id}"]`,
      )!,
    );
    const titleItems = titleButtons.map((button) => button.parentElement!);
    titleItems.forEach((item, index) => {
      item.getBoundingClientRect = () => new DOMRect(index * 100, 0, 100, 32);
      const captured = new Set<number>();
      item.setPointerCapture = (id) => captured.add(id);
      item.hasPointerCapture = (id) => captured.has(id);
      item.releasePointerCapture = (id) => captured.delete(id);
    });

    const pane = document.createElement("div");
    pane.dataset.paneId = "session-one";
    pane.getBoundingClientRect = () => new DOMRect(0, 40, 400, 400);
    document.body.append(pane);
    vi.spyOn(document, "elementFromPoint").mockReturnValue(pane);

    pointer(titleButtons[1], "pointerdown", 150, 16);
    pointer(window, "pointermove", 390, 200);
    expect(getExternalPaneDrop()).toMatchObject({
      fromId: "second",
      overId: "session-one",
      edge: "right",
    });
    pointer(window, "pointerup", 390, 200);

    expect(onPlaceOnPane).toHaveBeenCalledExactlyOnceWith(
      "second",
      "session-one",
      "right",
    );
    expect(onSelect).not.toHaveBeenCalled();
    expect(getExternalPaneDrop()).toBeNull();
    pane.remove();
  });

  it("resolves pane insertion on either side of a title tab", () => {
    act(() =>
      root.render(
        createElement(TitleBar, {
          tabs: [tab("first"), tab("second")],
          activeId: "first",
          cwd: "/project",
          onToggleSidebar: vi.fn(),
          onNew: vi.fn(),
          onSelect: vi.fn(),
          onClose: vi.fn(),
          onCloseMany: vi.fn(),
          onReorder: vi.fn(),
        }),
      ),
    );
    const second = container.querySelector<HTMLElement>(
      '[data-title-tab-id="second"]',
    )!;
    second.getBoundingClientRect = () => new DOMRect(100, 0, 100, 40);
    vi.spyOn(document, "elementFromPoint").mockReturnValue(second);

    expect(titleTabDropFromPoint(120, 20)).toEqual({
      targetTabId: "second",
      position: "before",
    });
    expect(titleTabDropFromPoint(180, 20)).toEqual({
      targetTabId: "second",
      position: "after",
    });
  });

  it("shows the insertion marker while a pane is over the title strip", () => {
    act(() =>
      root.render(
        createElement(TitleBar, {
          tabs: [tab("first"), tab("second")],
          activeId: "first",
          cwd: "/project",
          onToggleSidebar: vi.fn(),
          onNew: vi.fn(),
          onSelect: vi.fn(),
          onClose: vi.fn(),
          onCloseMany: vi.fn(),
          onReorder: vi.fn(),
        }),
      ),
    );

    act(() =>
      setExternalTitleTabDrop({
        fromId: "pane",
        targetTabId: "second",
        position: "before",
      }),
    );

    expect(
      container
        .querySelector("[data-pane-tab-drop-hint]")
        ?.closest("[data-title-tab-id]")
        ?.getAttribute("data-title-tab-id"),
    ).toBe("second");
  });
});
