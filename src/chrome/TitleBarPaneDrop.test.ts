// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { getExternalPaneDrop } from "../lib/paneDrop";
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
  act(() => root.unmount());
  container.remove();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("title tab pane drops", () => {
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
});
