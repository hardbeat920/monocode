// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { ComposerRunner } from "./ComposerRunner";

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

it("keeps animating through transcript changes without measuring layout each frame", () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal("matchMedia", () => ({ matches: false }));
  vi.spyOn(performance, "now").mockReturnValue(1000);
  let frame: FrameRequestCallback | null = null;
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
    frame = callback;
    return 1;
  });
  vi.stubGlobal("cancelAnimationFrame", () => {});

  const box = document.createElement("div");
  box.dataset.composer = "";
  const measure = vi.spyOn(box, "getBoundingClientRect").mockReturnValue({
    left: 20,
    right: 320,
    top: 400,
    bottom: 480,
    width: 300,
    height: 80,
  } as DOMRect);
  document.body.append(box);
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    act(() =>
      root.render(
        createElement(ComposerRunner, {
          boxRef: { current: box },
          cwd: "/work/project",
          busy: true,
          onExited: vi.fn(),
        }),
      ),
    );
    const startedAt = 1000;
    for (let i = 1; i <= 5; i++) {
      box.append(document.createElement("span"));
      act(() => frame?.(startedAt + i * 16));
    }
    expect(measure).toHaveBeenCalledOnce();

    act(() => frame?.(startedAt + 120));
    expect(measure).toHaveBeenCalledTimes(2);
  } finally {
    act(() => root.unmount());
    host.remove();
    box.remove();
  }
});

it("stops animating while its pane is in a background tab", () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal("matchMedia", () => ({ matches: false }));
  vi.spyOn(performance, "now").mockReturnValue(1000);
  let frame: FrameRequestCallback | null = null;
  const requestFrame = vi.fn((callback: FrameRequestCallback) => {
    frame = callback;
    return 1;
  });
  vi.stubGlobal("requestAnimationFrame", requestFrame);
  vi.stubGlobal("cancelAnimationFrame", () => {});

  const box = document.createElement("div");
  box.dataset.composer = "";
  vi.spyOn(box, "getBoundingClientRect").mockReturnValue({
    left: 20,
    right: 320,
    top: 400,
    bottom: 480,
    width: 300,
    height: 80,
  } as DOMRect);
  document.body.append(box);
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const onExited = vi.fn();
  const render = (enabled: boolean) =>
    act(() =>
      root.render(
        createElement(ComposerRunner, {
          boxRef: { current: box },
          cwd: "/work/project",
          busy: true,
          enabled,
          onExited,
        }),
      ),
    );
  try {
    render(true);
    act(() => frame?.(1016));
    const layer = document.body.lastElementChild as HTMLElement;
    expect(layer.style.display).toBe("");

    // Switching tabs disables the pane, which then stops re-rendering: `busy`
    // stays true there even after the turn ends.
    render(false);
    act(() => frame?.(1032));
    const requested = requestFrame.mock.calls.length;
    act(() => frame?.(1048));
    expect(requestFrame).toHaveBeenCalledTimes(requested);
    // Out of the render tree, so its looping CSS sprites stop too.
    expect(layer.style.display).toBe("none");

    render(true);
    expect(requestFrame).toHaveBeenCalledTimes(requested + 1);
    act(() => frame?.(5000));
    expect(layer.style.display).toBe("");
    expect(onExited).not.toHaveBeenCalled();
  } finally {
    act(() => root.unmount());
    host.remove();
    box.remove();
  }
});
