// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  EffortMeter,
  orderEffortOptions,
  type EffortTier,
} from "./EffortMeter";

let container: HTMLDivElement;
let root: Root;

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

const GROK_OPTIONS = [
  { value: "xhigh", label: "Extra High" },
  { value: "high", label: "High" },
  { value: "medium", label: "Medium" },
  { value: "low", label: "Low" },
];

const TIERS = orderEffortOptions(GROK_OPTIONS);

function renderMeter(props: {
  value: string;
  onChange?: (value: string) => void;
  onClose?: () => void;
  tiers?: EffortTier[];
  defaultValue?: string;
}) {
  act(() =>
    root.render(
      createElement(EffortMeter, {
        tiers: props.tiers ?? TIERS,
        value: props.value,
        defaultValue: props.defaultValue ?? "high",
        modelName: "Grok 4.6",
        onChange: props.onChange ?? vi.fn(),
        onClose: props.onClose ?? vi.fn(),
      }),
    ),
  );
  return container.querySelector<HTMLElement>('[role="slider"]')!;
}

function keyDown(target: EventTarget, key: string) {
  act(() => {
    target.dispatchEvent(
      new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true }),
    );
  });
}

describe("orderEffortOptions", () => {
  it("sorts catalog order into ascending effort", () => {
    expect(TIERS.map((tier) => tier.value)).toEqual([
      "low",
      "medium",
      "high",
      "xhigh",
    ]);
  });

  it("puts auto first and unknown values last", () => {
    const tiers = orderEffortOptions([
      { value: "high", label: "High" },
      { value: "turbo", label: "Turbo" },
      { value: "auto", label: "Auto" },
      { value: "low", label: "Low" },
    ]);
    expect(tiers.map((tier) => tier.value)).toEqual([
      "auto",
      "low",
      "high",
      "turbo",
    ]);
    expect(tiers[0].kind).toBe("auto");
  });
});

describe("EffortMeter", () => {
  it("commits on End and steps with the wheel", () => {
    const onChange = vi.fn();
    const slider = renderMeter({ value: "low", onChange });
    expect(slider.getAttribute("aria-valuemin")).toBe("0");
    expect(slider.getAttribute("aria-valuemax")).toBe("3");
    expect(slider.getAttribute("aria-valuenow")).toBe("0");
    expect(slider.getAttribute("aria-valuetext")).toBe("Low");

    keyDown(slider, "End");
    expect(onChange).toHaveBeenLastCalledWith("xhigh");

    // The committed value comes back through props; without a re-render the
    // slider still sits on "low", so a wheel-up steps to "medium".
    act(() => {
      slider.dispatchEvent(
        new WheelEvent("wheel", {
          deltaY: -40,
          bubbles: true,
          cancelable: true,
        }),
      );
    });
    expect(onChange).toHaveBeenLastCalledWith("medium");
  });

  it("only offers reset away from the default", () => {
    const onChange = vi.fn();
    renderMeter({ value: "high", onChange });
    const resetDefault = container.querySelector<HTMLButtonElement>(
      'button[aria-label="Reset to default"]',
    )!;
    expect(resetDefault.style.visibility).toBe("hidden");

    renderMeter({ value: "xhigh", onChange });
    const reset = container.querySelector<HTMLButtonElement>(
      'button[aria-label="Reset to default"]',
    )!;
    expect(reset.style.visibility).toBe("visible");
    act(() => reset.click());
    expect(onChange).toHaveBeenCalledWith("high");
  });

  it("marks the fill as top tier only at the last tier", () => {
    renderMeter({ value: "xhigh" });
    expect(container.querySelector(".effort-top-halo")).toBeNull();
    expect(
      container.querySelector(".effort-top-tint[data-top-tier]"),
    ).not.toBeNull();

    renderMeter({ value: "medium" });
    expect(container.querySelector("[data-top-tier]")).toBeNull();
  });

  it("reveals a full-width fill using the shared rail fraction", () => {
    const slider = renderMeter({ value: "medium" });
    const rail = container.querySelector<HTMLElement>(".effort-rail")!;
    const fill = container.querySelector<HTMLElement>(".effort-rail-fill")!;
    // "medium" is index 1 of 4 tiers → one third of the rail.
    expect(rail.style.getPropertyValue("--effort-frac")).toContain("0.333");
    expect(fill.style.width).toBe("");
    expect(rail.style.getPropertyValue("--effort-frac")).toContain("13.5px");
    expect(
      container.querySelectorAll(".effort-ticks .effort-rail-tick"),
    ).toHaveLength(4);

    expect(container.querySelector(".effort-specks-highlight")).not.toBeNull();
    expect(slider.hasAttribute("data-pressed")).toBe(false);
    act(() => {
      slider.dispatchEvent(
        new PointerEvent("pointerdown", {
          clientX: 50,
          bubbles: true,
          cancelable: true,
        }),
      );
    });
    expect(slider.hasAttribute("data-pressed")).toBe(true);
    act(() => {
      slider.dispatchEvent(
        new PointerEvent("pointerup", {
          clientX: 50,
          bubbles: true,
          cancelable: true,
        }),
      );
    });
    expect(slider.hasAttribute("data-pressed")).toBe(false);
  });

  it.each(["pointercancel", "lostpointercapture"])(
    "preserves pickup, interrupts settle, and restores the commit on %s",
    (cancelEvent) => {
      let now = 0;
      let id = 0;
      const frames = new Map<number, FrameRequestCallback>();
      vi.stubGlobal(
        "requestAnimationFrame",
        (callback: FrameRequestCallback) => {
          frames.set(++id, callback);
          return id;
        },
      );
      vi.stubGlobal("cancelAnimationFrame", (key: number) =>
        frames.delete(key),
      );
      const clock = vi.spyOn(performance, "now").mockImplementation(() => now);
      const advance = (ms: number) => {
        now += ms;
        const callbacks = [...frames.values()];
        frames.clear();
        act(() => callbacks.forEach((callback) => callback(now)));
      };
      const onChange = vi.fn();
      const slider = renderMeter({ value: "medium", onChange });
      const rail = container.querySelector<HTMLElement>(".effort-rail")!;
      vi.spyOn(rail, "getBoundingClientRect").mockReturnValue({
        left: 0,
        top: 0,
        width: 227,
        height: 22,
      } as DOMRect);
      const frac = () =>
        Number(
          rail.style
            .getPropertyValue("--effort-frac")
            .match(/\+ ([\d.]+) \*/)?.[1],
        );
      const pointer = (type: string, x: number) =>
        act(() => {
          slider.dispatchEvent(
            new PointerEvent(type, {
              pointerId: 1,
              clientX: x,
              clientY: 11,
              button: 0,
              bubbles: true,
              cancelable: true,
            }),
          );
        });
      const center = 13.5 + 200 / 3;
      pointer("pointerdown", center + 10);
      advance(16);
      expect(frac()).toBeCloseTo(1 / 3);
      pointer("pointermove", center + 30);
      // No event-rate rendering: the next frame drives all layers.
      expect(frac()).toBeCloseTo(1 / 3);
      advance(16);
      expect(frac()).toBeCloseTo(1 / 3 + 0.1);
      expect(
        Number(slider.style.getPropertyValue("--effort-light")),
      ).toBeGreaterThan(0.55);
      pointer(cancelEvent, center + 30);
      advance(200);
      expect(frac()).toBeCloseTo(1 / 3);
      expect(onChange).not.toHaveBeenCalled();
      expect(slider.hasAttribute("data-pressed")).toBe(false);
      expect(slider.style.getPropertyValue("--effort-sx")).toBe("1");

      // Bare rail presses use zero offset; release snaps without overshoot.
      pointer("pointerdown", 113.5);
      advance(16);
      expect(frac()).toBeCloseTo(0.5);
      pointer("pointerup", 113.5);
      expect(onChange).toHaveBeenLastCalledWith("high");
      expect(
        parseFloat(slider.style.getPropertyValue("--effort-settle-ms")),
      ).toBeCloseTo(110 + 2 * (200 / 6));
      renderMeter({ value: "high", onChange });
      advance(40);
      const interrupted = frac();
      expect(interrupted).toBeGreaterThan(0.5);
      expect(interrupted).toBeLessThan(2 / 3);
      const pickup = 13.5 + 200 * interrupted + 8;
      pointer("pointerdown", pickup);
      advance(16);
      expect(frac()).toBeCloseTo(interrupted);
      pointer("pointermove", 1000);
      advance(16);
      expect(frac()).toBe(1);
      pointer("pointermove", -1000);
      advance(16);
      expect(frac()).toBe(0);
      pointer(cancelEvent, -1000);
      advance(200);
      expect(frac()).toBeCloseTo(2 / 3);
      expect(onChange).toHaveBeenCalledTimes(1);

      keyDown(slider, "Home");
      renderMeter({ value: "low", onChange });
      expect(slider.style.getPropertyValue("--effort-settle-ms")).toBe("140ms");
      let previous = frac();
      for (let i = 0; i < 9; i++) {
        advance(16);
        expect(frac()).toBeLessThanOrEqual(previous);
        expect(frac()).toBeGreaterThanOrEqual(0);
        previous = frac();
      }
      expect(frac()).toBe(0);
      pointer("pointerdown", 13.5);
      advance(16);
      pointer("pointerup", 13.5);
      expect(slider.style.getPropertyValue("--effort-settle-ms")).toBe("0ms");
      clock.mockRestore();
    },
  );

  it("uses a balanced static field with only four twinkling sites", () => {
    renderMeter({ value: "high" });
    const field = container.querySelector(".effort-specks")!;
    const sites = [...field.querySelectorAll<HTMLElement>(".effort-speck")];
    expect(sites).toHaveLength(30);
    expect(sites.filter((site) => site.style.width === "1px")).toHaveLength(20);
    expect(sites.filter((site) => site.style.width === "1.5px")).toHaveLength(
      8,
    );
    expect(sites.filter((site) => site.style.width === "2px")).toHaveLength(2);
    expect(field.querySelectorAll("[data-twinkle]")).toHaveLength(4);
    expect(field.querySelectorAll("[data-drift]")).toHaveLength(0);
  });

  it("closes on Enter", () => {
    const onClose = vi.fn();
    const slider = renderMeter({ value: "high", onClose });
    keyDown(slider, "Enter");
    expect(onClose).toHaveBeenCalled();
  });
});
