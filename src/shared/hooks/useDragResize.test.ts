// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useDragResize } from "./useDragResize";

function Harness({
  initial,
  onCommit,
}: {
  initial: number;
  onCommit?: (width: number) => void;
}) {
  const resize = useDragResize({
    min: 100,
    max: () => 400,
    defaultWidth: 150,
    initial,
    onCommit,
  });
  return createElement(
    "div",
    null,
    createElement("div", {
      ref: resize.setPaneRef,
      "data-testid": "pane",
    }),
    createElement("div", {
      "data-testid": "handle",
      onPointerDown: resize.onPointerDown,
    }),
    createElement("span", { "data-testid": "width" }, String(resize.width)),
  );
}

describe("useDragResize", () => {
  let root: Root;
  let container: HTMLDivElement;
  let commits: number[];

  beforeEach(() => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    commits = [];
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

  function render(initial: number, onCommit?: (width: number) => void) {
    act(() =>
      root.render(createElement(Harness, { initial, onCommit })),
    );
  }

  function pointer(target: EventTarget, type: string, clientX: number) {
    act(() =>
      target.dispatchEvent(
        new PointerEvent(type, {
          bubbles: true,
          cancelable: true,
          button: 0,
          pointerId: 1,
          clientX,
          clientY: 0,
        }),
      ),
    );
  }

  async function drag(toX: number) {
    const handle = container.querySelector<HTMLElement>(
      '[data-testid="handle"]',
    )!;
    const captured = handle as unknown as {
      setPointerCapture: (id: number) => void;
      releasePointerCapture: (id: number) => void;
    };
    captured.setPointerCapture = () => {};
    captured.releasePointerCapture = () => {};
    pointer(handle, "pointerdown", 300);
    pointer(window, "pointermove", toX);
    pointer(window, "pointerup", toX);
    await act(async () => {});
  }

  function width() {
    return container.querySelector<HTMLElement>('[data-testid="width"]')!
      .textContent;
  }

  it("keeps the dragged width when no commit callback is provided", async () => {
    render(200);
    await drag(400);

    expect(commits).toEqual([]);
    expect(width()).toBe("300");

    // An unrelated re-render with the same initial must not snap it back.
    render(200);
    expect(width()).toBe("300");
  });

  it("commits the drag width to the caller", async () => {
    render(200, (next) => commits.push(next));
    await drag(400);

    expect(commits).toEqual([300]);
    expect(width()).toBe("300");
  });

  it("adopts an initial change made elsewhere", async () => {
    render(200);
    render(350);
    expect(width()).toBe("350");
  });
});
