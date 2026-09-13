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
