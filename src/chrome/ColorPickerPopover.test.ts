// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ColorPickerPopover } from "./ColorPickerPopover";

let container: HTMLDivElement;
let root: ReturnType<typeof createRoot>;

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

function key(target: Element, value: string, shiftKey = false) {
  act(() => {
    target.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: value,
        shiftKey,
        bubbles: true,
        cancelable: true,
      }),
    );
  });
}

describe("ColorPickerPopover keyboard controls", () => {
  it("focuses the hex input when requested", () => {
    act(() => {
      root.render(
        createElement(ColorPickerPopover, {
          value: "#ff0000",
          onChange: vi.fn(),
          autoFocus: true,
        }),
      );
    });

    expect(document.activeElement).toBe(
      container.querySelector('input[aria-label="Hex color"]'),
    );
  });

  it("updates saturation, brightness, and hue with arrow keys", () => {
    const onChange = vi.fn();
    act(() => {
      root.render(
        createElement(ColorPickerPopover, {
          value: "#ff0000",
          onChange,
        }),
      );
    });

    const saturation = container.querySelector(
      '[role="slider"][aria-label="Saturation and brightness"]',
    )!;
    const hue = container.querySelector('[role="slider"][aria-label="Hue"]')!;
    key(saturation, "ArrowLeft");
    expect(saturation.getAttribute("aria-valuenow")).toBe("99");
    key(saturation, "ArrowDown", true);
    expect(saturation.getAttribute("aria-valuetext")).toContain(
      "90% brightness",
    );
    key(hue, "ArrowRight");
    expect(hue.getAttribute("aria-valuenow")).toBe("1");
    expect(onChange).toHaveBeenCalledTimes(3);
  });
});

describe("ColorPickerPopover pointer controls", () => {
  it.each(["Saturation and brightness", "Hue"])(
    "stops the %s drag when unmounted",
    (label) => {
      const onChange = vi.fn();
      const addListener = vi.spyOn(window, "addEventListener");
      const removeListener = vi.spyOn(window, "removeEventListener");
      act(() => {
        root.render(
          createElement(ColorPickerPopover, {
            value: "#ff0000",
            onChange,
          }),
        );
      });

      const slider = container.querySelector<HTMLElement>(
        `[role="slider"][aria-label="${label}"]`,
      )!;
      vi.spyOn(slider, "getBoundingClientRect").mockReturnValue({
        left: 0,
        top: 0,
        width: 100,
        height: 100,
        right: 100,
        bottom: 100,
        x: 0,
        y: 0,
        toJSON: () => ({}),
      });
      vi.spyOn(slider, "setPointerCapture").mockImplementation(() => {});

      act(() => {
        slider.dispatchEvent(
          new PointerEvent("pointerdown", {
            pointerId: 1,
            clientX: 50,
            clientY: 50,
            bubbles: true,
            cancelable: true,
          }),
        );
      });
      onChange.mockClear();
      const pointerTypes = [
        "pointermove",
        "pointerup",
        "pointercancel",
      ] as const;
      const pointerListeners = new Map(
        addListener.mock.calls.filter(([type]) =>
          pointerTypes.includes(type as (typeof pointerTypes)[number]),
        ),
      );
      expect(pointerListeners.size).toBe(pointerTypes.length);

      act(() => root.render(null));
      for (const type of pointerTypes) {
        expect(removeListener).toHaveBeenCalledWith(
          type,
          pointerListeners.get(type),
        );
      }
      window.dispatchEvent(
        new PointerEvent("pointermove", { clientX: 75, clientY: 25 }),
      );

      expect(onChange).not.toHaveBeenCalled();
    },
  );
});
