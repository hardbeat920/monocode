// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../files/editor/syntaxTokens", () => ({
  // Syntax colours never arrive; these tests only look at scrolling.
  highlightDiffFile: vi.fn(() => new Promise(() => {})),
}));

import { UnifiedDiffView, type UnifiedDiffFileModel } from "./UnifiedDiffView";

const SCROLLER_TOP = 40;
let root: Root;
let container: HTMLDivElement;
let scrollTop = 0;
let heights: Record<string, number> = {};
let order: string[] = [];
let notifyResize: () => void = () => {};
const scrollTo = vi.fn();

function file(id: string): UnifiedDiffFileModel {
  return {
    id,
    path: `/r/${id}.ts`,
    label: `${id}.ts`,
    additions: 1,
    deletions: 0,
    blocks: [],
  };
}

/** Lay the files out one after another inside the scroller. */
function offsetOf(path: string | undefined): number {
  let offset = 0;
  for (const id of order) {
    if (`/r/${id}.ts` === path) return offset;
    offset += heights[id] ?? 0;
  }
  return offset;
}

function isScroller(node: Element) {
  return node.classList.contains("unified-diff");
}

function scroller() {
  const node = container.querySelector<HTMLElement>(".unified-diff");
  if (!node) throw new Error("missing scroller");
  return node;
}

function render(ids: string[], focusId?: string, focusRequest?: number) {
  order = ids;
  act(() =>
    root.render(
      createElement(UnifiedDiffView, {
        files: ids.map(file),
        focusId,
        focusRequest,
      }),
    ),
  );
}

/** Diffs above the selection finish loading and push it down. */
function grow(id: string, height: number) {
  heights[id] = height;
  act(() => notifyResize());
}

function lastScroll() {
  return scrollTo.mock.lastCall?.[0]?.top;
}

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
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
  vi.stubGlobal(
    "IntersectionObserver",
    class {
      observe() {}
      disconnect() {}
    },
  );
  scrollTop = 0;
  heights = {};
  order = [];
  scrollTo.mockReset();
  scrollTo.mockImplementation((options: ScrollToOptions) => {
    scrollTop = options.top ?? scrollTop;
  });
  Object.defineProperty(HTMLElement.prototype, "scrollTop", {
    configurable: true,
    get(this: HTMLElement) {
      return isScroller(this) ? scrollTop : 0;
    },
    set(this: HTMLElement, value: number) {
      if (isScroller(this)) scrollTop = value;
    },
  });
  vi.spyOn(HTMLElement.prototype, "scrollTo").mockImplementation(function (
    this: HTMLElement,
    options?: ScrollToOptions | number,
  ) {
    if (isScroller(this) && typeof options === "object") scrollTo(options);
  } as HTMLElement["scrollTo"]);
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
    function (this: HTMLElement) {
      const top = isScroller(this)
        ? SCROLLER_TOP
        : this.dataset.diffFile
          ? SCROLLER_TOP + offsetOf(this.dataset.diffFile) - scrollTop
          : 0;
      return { top, bottom: top, left: 0, right: 0, height: 0, width: 0 } as DOMRect;
    },
  );
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  delete (HTMLElement.prototype as { scrollTop?: number }).scrollTop;
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("UnifiedDiffView focus", () => {
  it("keeps the selected file at the top while diffs above it load", () => {
    heights = { a: 20, b: 20, c: 20 };
    render(["a", "b", "c"], "c");
    expect(lastScroll()).toBe(40);

    grow("a", 300);
    expect(lastScroll()).toBe(320);
    grow("b", 500);
    expect(lastScroll()).toBe(800);
    expect(scroller().scrollTop).toBe(800);
  });

  it("scrolls back to the file when it is selected again", () => {
    heights = { a: 100, b: 100, c: 100 };
    render(["a", "b", "c"], "b", 1);
    expect(lastScroll()).toBe(100);

    act(() => {
      scroller().dispatchEvent(new WheelEvent("wheel", { bubbles: true }));
    });
    scrollTop = 0;
    scrollTo.mockClear();

    render(["a", "b", "c"], "b", 2);
    expect(scrollTo).toHaveBeenCalledTimes(1);
    expect(lastScroll()).toBe(100);

    // Following resumes after the repeated pick.
    grow("a", 400);
    expect(lastScroll()).toBe(400);
  });

  it.each([
    ["wheel", () => new WheelEvent("wheel", { bubbles: true })],
    ["pointerdown", () => new PointerEvent("pointerdown", { bubbles: true })],
    ["keydown", () => new KeyboardEvent("keydown", { bubbles: true })],
  ])("stops following the file after a %s in the view", (_name, event) => {
    heights = { a: 20, b: 20, c: 20 };
    render(["a", "b", "c"], "c");
    expect(lastScroll()).toBe(40);

    act(() => {
      scroller().dispatchEvent(event());
    });
    scrollTo.mockClear();

    grow("a", 300);
    expect(scrollTo).not.toHaveBeenCalled();

    // A refreshed file list doesn't pull the reader back either.
    render(["z", "a", "b", "c"], "c");
    expect(scrollTo).not.toHaveBeenCalled();
  });
});
