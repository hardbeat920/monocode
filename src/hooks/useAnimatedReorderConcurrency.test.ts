// @vitest-environment happy-dom
import { act, createElement, startTransition, Suspense } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useAnimatedReorder } from "./useAnimatedReorder";

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
  document.documentElement.style.setProperty(
    "--motion-reorder-duration",
    "160ms",
  );
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  document.documentElement.style.removeProperty("--motion-reorder-duration");
  vi.clearAllTimers();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

function pointer(target: EventTarget, type: string, clientX: number) {
  act(() => {
    target.dispatchEvent(
      new PointerEvent(type, {
        bubbles: true,
        button: 0,
        pointerId: 1,
        clientX,
        clientY: 16,
      }),
    );
  });
}

describe("reordering during an uncommitted render", () => {
  it.each(["before the drag", "while settling"])(
    "uses the committed order and callback when rendering suspends %s",
    async (timing) => {
      const onReorder = vi.fn();
      const pendingOnReorder = vi.fn();
      const pendingRender = vi.fn();
      const suspended = new Promise<never>(() => {});

      function List({ pending }: { pending: boolean }) {
        const ids = pending ? ["a", "b", "c", "pending"] : ["a", "b", "c"];
        const reorder = useAnimatedReorder(
          ids,
          pending ? pendingOnReorder : onReorder,
        );
        if (pending) {
          pendingRender();
          throw suspended;
        }
        return createElement(
          "div",
          { "data-list": true },
          ids.map((id, index) =>
            createElement(
              "button",
              {
                key: id,
                ref: (node: HTMLButtonElement | null) => {
                  if (node) {
                    node.getBoundingClientRect = () =>
                      new DOMRect(index * 100, 0, 100, 32);
                    const captured = new Set<number>();
                    node.setPointerCapture = (pointerId) => {
                      captured.add(pointerId);
                    };
                    node.hasPointerCapture = (pointerId) =>
                      captured.has(pointerId);
                    node.releasePointerCapture = (pointerId) => {
                      captured.delete(pointerId);
                    };
                  }
                  reorder.setItemRef(id, node);
                },
                onPointerDown: (event) => reorder.onItemPointerDown(id, event),
              },
              id,
            ),
          ),
        );
      }

      const render = (pending: boolean) =>
        root.render(
          createElement(
            Suspense,
            { fallback: "Loading" },
            createElement(List, { pending }),
          ),
        );
      const drag = () => {
        pointer(container.querySelector("button")!, "pointerdown", 50);
        pointer(window, "pointermove", 160);
        pointer(window, "pointerup", 160);
      };

      await act(async () => render(false));
      if (timing === "while settling") drag();
      await act(async () => startTransition(() => render(true)));
      expect(pendingRender).toHaveBeenCalled();
      expect(container.querySelector("[data-list]")?.textContent).toBe("abc");
      if (timing === "before the drag") drag();
      act(() => vi.advanceTimersByTime(160));

      expect(onReorder).toHaveBeenCalledExactlyOnceWith(["b", "a", "c"], "a");
      expect(pendingOnReorder).not.toHaveBeenCalled();
    },
  );
});
