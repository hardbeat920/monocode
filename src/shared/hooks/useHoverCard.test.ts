// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  useHoverCard,
  type HoverCardController,
  type HoverCardTiming,
} from "./useHoverCard";

let container: HTMLDivElement;
let root: Root;
let alive = false;
let controller: HoverCardController;

function mount(timing?: HoverCardTiming) {
  function Probe() {
    controller = useHoverCard(timing);
    return null;
  }
  act(() => root.render(createElement(Probe)));
}

function unmount() {
  act(() => root.unmount());
  alive = false;
}

function advance(ms: number) {
  act(() => {
    vi.advanceTimersByTime(ms);
  });
}

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.useFakeTimers();
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
  alive = true;
});

afterEach(() => {
  // A test may have unmounted already; unmounting twice would throw.
  if (alive) unmount();
  container.remove();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe("useHoverCard", () => {
  it("stays closed until the open delay elapses", () => {
    mount({ openDelayMs: 200 });
    act(() => controller.openAfterDelay());
    expect(controller.open).toBe(false);

    advance(199);
    expect(controller.open).toBe(false);

    advance(1);
    expect(controller.open).toBe(true);
  });

  it("opens at once for focus and closes at once on blur", () => {
    mount();
    act(() => controller.openNow());
    expect(controller.open).toBe(true);

    act(() => controller.closeNow());
    expect(controller.open).toBe(false);
  });

  it("does not restart the open timer on repeated hovers", () => {
    mount({ openDelayMs: 200 });
    act(() => controller.openAfterDelay());
    advance(150);
    act(() => controller.openAfterDelay());

    advance(50);
    expect(controller.open).toBe(true);
  });

  it("delays closing so the pointer can move onto the card", () => {
    mount({ closeDelayMs: 100 });
    act(() => controller.openNow());
    act(() => controller.closeAfterDelay());

    advance(99);
    expect(controller.open).toBe(true);
    advance(1);
    expect(controller.open).toBe(false);
  });

  it("cancels a pending close when the pointer re-enters", () => {
    mount({ closeDelayMs: 100 });
    act(() => controller.openNow());
    act(() => controller.closeAfterDelay());
    advance(50);
    act(() => controller.cancelClose());

    advance(500);
    expect(controller.open).toBe(true);
  });

  it("cancels a pending open on an immediate close", () => {
    mount({ openDelayMs: 200 });
    act(() => controller.openAfterDelay());
    act(() => controller.closeNow());

    advance(500);
    expect(controller.open).toBe(false);
  });

  it("cancels a pending close on an immediate open", () => {
    mount({ closeDelayMs: 100 });
    act(() => controller.openNow());
    act(() => controller.closeAfterDelay());
    act(() => controller.openNow());

    advance(500);
    expect(controller.open).toBe(true);
  });

  it("clears pending timers on unmount", () => {
    mount({ openDelayMs: 200, closeDelayMs: 100 });
    act(() => controller.openAfterDelay());
    act(() => controller.closeAfterDelay());
    expect(vi.getTimerCount()).toBeGreaterThan(0);

    unmount();
    expect(vi.getTimerCount()).toBe(0);
  });

  it("does not re-arm the open timer when the delay prop changes", () => {
    // Timers are refs, so a re-render must not restart the countdown. The
    // commit card passes a 600ms delay; if the timer were state-driven, every
    // parent re-render would push the open further away.
    function Probe({ delay }: { delay: number }) {
      controller = useHoverCard({ openDelayMs: delay });
      return null;
    }
    act(() => root.render(createElement(Probe, { delay: 200 })));
    act(() => controller.openAfterDelay());

    act(() => root.render(createElement(Probe, { delay: 600 })));
    advance(199);
    expect(controller.open).toBe(false);
    advance(1);
    expect(controller.open).toBe(true);
  });

  it("keeps the callbacks stable across re-renders", () => {
    function Probe() {
      controller = useHoverCard();
      return null;
    }
    act(() => root.render(createElement(Probe)));
    const first = controller.openAfterDelay;
    act(() => controller.openNow());
    act(() => root.render(createElement(Probe)));
    // A caller passing these straight to a DOM prop gets no needless listener
    // churn when unrelated state changes.
    expect(controller.openAfterDelay).toBe(first);
  });
});
