// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ColorPickerPopover } from "./ColorPickerPopover";

describe("ColorPickerPopover drag cleanup", () => {
  let root: Root;
  let container: HTMLDivElement;

  beforeEach(() => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    container.remove();
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it("removes every active drag listener on unmount mid-drag", async () => {
    const added: Array<{ type: string; fn: EventListener }> = [];
    const removed: EventListener[] = [];
    const rawAdd = window.addEventListener.bind(window);
    const rawRemove = window.removeEventListener.bind(window);
    vi.spyOn(window, "addEventListener").mockImplementation(
      ((type: string, fn: EventListener, opts?: object) => {
        added.push({ type, fn });
        return rawAdd(type, fn as EventListener, opts);
      }) as typeof window.addEventListener,
    );
    vi.spyOn(window, "removeEventListener").mockImplementation(
      ((type: string, fn: EventListener, opts?: object) => {
        removed.push(fn);
        return rawRemove(type, fn as EventListener, opts);
      }) as typeof window.removeEventListener,
    );
    await act(async () => {
      root.render(
        createElement(ColorPickerPopover, {
          value: "#ff0000",
          onChange: () => {},
        }),
      );
    });
    const sliders = container.querySelectorAll('[role="slider"]');
    expect(sliders.length).toBe(2);
    const firePointerDown = (el: Element, pointerId: number) => {
      (el as HTMLElement).setPointerCapture = () => {};
      (el as HTMLElement).getBoundingClientRect = () =>
        ({ left: 0, top: 0, width: 100, height: 100 }) as DOMRect;
      el.dispatchEvent(
        new PointerEvent("pointerdown", {
          bubbles: true,
          pointerId,
          clientX: 10,
          clientY: 10,
        }),
      );
    };
    act(() => {
      firePointerDown(sliders[0]!, 1);
      firePointerDown(sliders[1]!, 2);
    });
    const moves = added.filter((entry) => entry.type === "pointermove");
    expect(moves.length).toBe(2);
    await act(async () => root.unmount());
    for (const entry of moves) {
      expect(removed).toContain(entry.fn);
    }
  });
});
