// @vitest-environment happy-dom
import { act, createElement, useRef } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { useQuickPickerMotion } from "./useQuickPickerMotion";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockResolvedValue(undefined),
}));

type Picker = "model" | "project" | null;
let root: Root;
let container: HTMLDivElement;
let notifyResize: () => void;
let reduced = false;
let sampledHeight: number | null = null;
const animations: Array<{
  cancel: ReturnType<typeof vi.fn>;
  onfinish: (() => void) | null;
}> = [];
const animate = vi.fn(
  (_frames: Keyframe[], _options: KeyframeAnimationOptions) => {
    const animation = {
      cancel: vi.fn(),
      onfinish: null as (() => void) | null,
    };
    animations.push(animation);
    return animation;
  },
);

function Harness({ picker }: { picker: Picker }) {
  const frame = useRef<HTMLDivElement>(null);
  const panel = useRef<HTMLDivElement>(null);
  useQuickPickerMotion(frame, panel, picker);
  return createElement(
    "div",
    { ref: frame, "data-frame": "", "data-picker": picker ?? "" },
    picker
      ? createElement("div", { ref: panel, key: picker, "data-picker": picker })
      : null,
  );
}
function render(picker: Picker) {
  act(() => root.render(createElement(Harness, { picker })));
}

beforeEach(() => {
  vi.mocked(invoke).mockResolvedValue(undefined);
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal("matchMedia", () => ({ matches: reduced }));
  vi.stubGlobal(
    "ResizeObserver",
    class {
      constructor(callback: () => void) {
        notifyResize = callback;
      }
      observe() {}
      disconnect() {}
    },
  );
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
    function (this: HTMLElement) {
      const panelHeight =
        this.dataset.picker === "model"
          ? 400
          : this.dataset.picker === "project"
            ? 180
            : 0;
      return {
        height: this.hasAttribute("data-frame")
          ? (sampledHeight ?? 100 + panelHeight)
          : panelHeight,
      } as DOMRect;
    },
  );
  vi.stubGlobal("Animation", class {});
  vi.spyOn(HTMLElement.prototype, "animate").mockImplementation(
    animate as never,
  );
  reduced = false;
  sampledHeight = null;
  animations.length = 0;
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  vi.clearAllMocks();
});

it.each(["model", "project"] as const)(
  "expands and fades the %s picker while resizing the native window",
  (picker) => {
    render(null);
    expect(animate).not.toHaveBeenCalled();
    render(picker);
    expect(animate.mock.calls[0][0]).toEqual([
      { height: "100px" },
      { height: picker === "model" ? "500px" : "280px" },
    ]);
    expect(animate.mock.calls[1][0]).toEqual([{ opacity: 0 }, { opacity: 1 }]);
    sampledHeight = 220;
    act(() => notifyResize());
    expect(invoke).toHaveBeenLastCalledWith("quick_composer_fit", {
      height: 220,
    });
    act(() => animations[0].onfinish?.());
    expect(
      container.querySelector<HTMLElement>("[data-frame] > div")?.style.height,
    ).toBe("");
  },
);

it("cancels an interrupted expansion and starts from its current height", () => {
  render(null);
  render("model");
  sampledHeight = 320;
  act(() => notifyResize());
  sampledHeight = null;
  render("project");
  expect(animations[0].cancel).toHaveBeenCalled();
  expect(animations[1].cancel).toHaveBeenCalled();
  expect(animate.mock.calls[2][0]).toEqual([
    { height: "320px" },
    { height: "280px" },
  ]);
});

it("fits immediately without animation when reduced motion is enabled", () => {
  reduced = true;
  render(null);
  render("model");
  act(() => notifyResize());
  expect(animate).not.toHaveBeenCalled();
  expect(invoke).toHaveBeenLastCalledWith("quick_composer_fit", {
    height: 500,
  });
});
