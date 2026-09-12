import { createElement, type PointerEvent as ReactPointerEvent } from "react";
import { renderToString } from "react-dom/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useAnimatedReorder } from "./useAnimatedReorder";

const ids = ["sessions", "changes", "explorer"];

function tabAt(left: number, top: number) {
  const captured = new Set<number>();
  return {
    getBoundingClientRect: () => ({
      left,
      right: left + 100,
      width: 100,
      top,
      bottom: top + 32,
      height: 32,
    }),
    style: {
      transform: "",
      transition: "",
      removeProperty(key: "transform" | "transition") {
        this[key] = "";
      },
    },
    dataset: {} as Record<string, string>,
    setPointerCapture: (id: number) => captured.add(id),
    hasPointerCapture: (id: number) => captured.has(id),
    releasePointerCapture: (id: number) => captured.delete(id),
  };
}

let browser: EventTarget;

function pointer(type: string, clientX: number, pointerId = 1) {
  browser.dispatchEvent(
    Object.assign(new Event(type), { clientX, clientY: clientX, pointerId }),
  );
}

function setup(reducedMotion = false, axis: "x" | "y" = "x") {
  Object.assign(browser, { matchMedia: () => ({ matches: reducedMotion }) });
  const onReorder = vi.fn();
  let reorder!: ReturnType<typeof useAnimatedReorder<string>>;
  // Render the real hook to obtain its gesture interface without mocking React.
  // Mount/unmount effects and native click targeting need a browser check.
  function Probe() {
    reorder = useAnimatedReorder(ids, onReorder, axis);
    return null;
  }
  renderToString(createElement(Probe));
  const tabs = [tabAt(0, 0), tabAt(100, 33), tabAt(200, 66)];
  tabs.forEach((tab, index) =>
    reorder.setItemRef(ids[index], tab as unknown as HTMLElement),
  );
  const press = (index = 0, pointerId = 1) =>
    reorder.onItemPointerDown(ids[index], {
      button: 0,
      clientX: index * 100 + 50,
      clientY: index * 33 + 16,
      pointerId,
    } as ReactPointerEvent);
  return { reorder, tabs, onReorder, press };
}

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "performance"] });
  browser = Object.assign(new EventTarget(), {
    getComputedStyle: () => ({
      getPropertyValue: (name: string) =>
        name === "--motion-reorder-duration"
          ? "160ms"
          : "cubic-bezier(0.22, 1, 0.36, 1)",
    }),
    getSelection: () => null,
    clearTimeout,
    requestAnimationFrame: (callback: () => void) => setTimeout(callback, 16),
    cancelAnimationFrame: clearTimeout,
  });
  const classes = new Set<string>();
  vi.stubGlobal("window", browser);
  vi.stubGlobal("document", {
    body: { style: { cursor: "" } },
    documentElement: {
      classList: {
        add: (name: string) => classes.add(name),
        remove: (name: string) => classes.delete(name),
      },
    },
  });
});

afterEach(() => {
  browser.dispatchEvent(new Event("blur"));
  vi.runAllTimers();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe("workspace tab gestures", () => {
  it("releases drag feedback immediately while the drop animation finishes", () => {
    const { press, tabs, onReorder } = setup();
    press();
    pointer("pointermove", 180);
    vi.advanceTimersByTime(16);
    expect(tabs[0].dataset.dragging).toBe("true");
    pointer("pointerup", 180);
    expect(tabs[0].dataset.dragging).toBeUndefined();
    expect(tabs[0].hasPointerCapture(1)).toBe(false);
    expect(document.body.style.cursor).toBe("");
    expect(onReorder).not.toHaveBeenCalled();
    vi.runAllTimers();
    expect(onReorder).toHaveBeenCalledExactlyOnceWith(
      ["changes", "sessions", "explorer"],
      "sessions",
    );
  });

  it("previews and reorders vertical project rows", () => {
    const { press, tabs, onReorder } = setup(false, "y");
    press();
    pointer("pointermove", 60);
    vi.advanceTimersByTime(16);
    expect(tabs[0].style.transform).toBe("translate3d(0, 44px, 0)");
    expect(tabs[1].style.transform).toBe("translate3d(0, -33px, 0)");
    expect(onReorder).not.toHaveBeenCalled();
    pointer("pointerup", 90);
    vi.runAllTimers();
    expect(onReorder).toHaveBeenCalledExactlyOnceWith(
      ["changes", "explorer", "sessions"],
      "sessions",
    );
  });

  it("preserves button clicks below the drag threshold", () => {
    const { press, tabs, reorder, onReorder } = setup();
    press();
    expect(tabs[0].hasPointerCapture(1)).toBe(false);
    pointer("pointermove", 53);
    expect(tabs[0].hasPointerCapture(1)).toBe(false);
    pointer("pointerup", 53);
    expect(reorder.consumeClick()).toBe(false);
    expect(onReorder).not.toHaveBeenCalled();
  });

  it("reorders on drop and permits a fresh click immediately afterwards", () => {
    const { press, tabs, reorder, onReorder } = setup();
    press();
    pointer("pointermove", 160);
    expect(tabs[0].hasPointerCapture(1)).toBe(true);
    expect(onReorder).not.toHaveBeenCalled();
    pointer("pointerup", 160);
    expect(reorder.consumeClick()).toBe(true);
    vi.runAllTimers();
    expect(onReorder).toHaveBeenCalledExactlyOnceWith(
      ["changes", "sessions", "explorer"],
      "sessions",
    );
    expect(tabs[0].hasPointerCapture(1)).toBe(false);
    press(2);
    pointer("pointerup", 250);
    expect(reorder.consumeClick()).toBe(false);
  });

  it("previews the latest pointer position and makes room before committing", () => {
    const { press, tabs, onReorder } = setup();
    press();
    pointer("pointermove", 100);
    pointer("pointermove", 180);
    vi.advanceTimersByTime(16);
    expect(tabs[0].style.transform).toBe("translate3d(130px, 0, 0)");
    expect(tabs[1].style.transform).toBe("translate3d(-100px, 0, 0)");
    expect(tabs[2].style.transform).toBe("translate3d(0px, 0, 0)");
    expect(onReorder).not.toHaveBeenCalled();
  });

  it("moves the last tab to the first slot when released beyond the left edge", () => {
    const { press, onReorder } = setup();
    press(2);
    pointer("pointerup", -100);
    vi.runAllTimers();
    expect(onReorder).toHaveBeenCalledExactlyOnceWith(
      ["explorer", "sessions", "changes"],
      "explorer",
    );
  });

  it.each(["Escape", "pointercancel", "blur"])(
    "cancels a drag on %s",
    (reason) => {
      const { press, tabs, onReorder } = setup();
      press();
      pointer("pointermove", 250);
      vi.advanceTimersByTime(16);
      const event =
        reason === "Escape"
          ? Object.assign(new Event("keydown", { cancelable: true }), {
              key: "Escape",
            })
          : new Event(reason);
      browser.dispatchEvent(event);
      pointer("pointerup", 250);
      vi.runAllTimers();
      expect(onReorder).not.toHaveBeenCalled();
      expect(tabs.every((tab) => tab.style.transform === "")).toBe(true);
      expect(tabs[0].hasPointerCapture(1)).toBe(false);
      expect(document.body.style.cursor).toBe("");
      if (reason === "Escape") expect(event.defaultPrevented).toBe(true);
    },
  );

  it("commits a fast drop without waiting for animation in reduced motion", () => {
    const { press, tabs, onReorder } = setup(true);
    press();
    pointer("pointerup", 250);
    expect(onReorder).toHaveBeenCalledExactlyOnceWith(
      ["changes", "explorer", "sessions"],
      "sessions",
    );
    expect(tabs.every((tab) => tab.style.transform === "")).toBe(true);
    expect(vi.getTimerCount()).toBe(0);
  });
});
