// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { BackgroundActivity } from "./BackgroundActivity";

let root: Root;
let container: HTMLDivElement;
const props = { tasks: ["pi-subagents"], busy: true, visible: true };

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(0);
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal("matchMedia", () => ({ matches: false }));
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});
function render(
  overrides: Partial<Parameters<typeof BackgroundActivity>[0]> = {},
  key = "turn",
) {
  act(() =>
    root.render(
      createElement(BackgroundActivity, { ...props, ...overrides, key }),
    ),
  );
}
function advance(ms: number) {
  act(() => vi.advanceTimersByTime(ms));
}
const elapsed = () =>
  container.querySelector(".background-activity-time")?.textContent;

it("starts each background run at zero and replaces the old tasks", () => {
  render();
  advance(35_000);
  expect(elapsed()).toBe("0:35");
  render({ busy: false, tasks: [], interrupted: true });
  advance(10_000);
  expect(elapsed()).toBe("0:35");
  render({ tasks: ["new-work"] });
  expect(elapsed()).toBe("0:00");
  expect(container.querySelector("section")?.dataset.phase).toBe("waiting");
  expect(container.querySelector("ul")?.textContent).toBe("new-work");
  advance(1_000);
  expect(elapsed()).toBe("0:01");
});

it("keeps the timer during task updates and continuation, and resets on another wait", () => {
  render();
  advance(12_000);
  render({ tasks: ["updated-work"] });
  expect(elapsed()).toBe("0:12");
  render({ tasks: [] });
  expect(container.querySelector("section")?.dataset.phase).toBe("continuing");
  advance(2_000);
  expect(elapsed()).toBe("0:14");
  render({ tasks: ["next-work"] });
  expect(elapsed()).toBe("0:00");
});

it("does not show another turn's ended panel", () => {
  render();
  render({ busy: false, tasks: [] });
  expect(container.querySelector("section")).not.toBeNull();
  render({ busy: false, tasks: [] }, "next-turn");
  expect(container.querySelector("section")).toBeNull();
});

it("announces stopping immediately while the visual ticker finishes a transition", () => {
  render();
  render({ tasks: [] });
  // The ticker now holds its previous frame for up to 340ms.
  render({ busy: false, tasks: [], interrupted: true });
  const live = container.querySelector('[role="status"]');
  expect(live?.textContent).toBe("Turn stopped");
  expect(live?.closest('[aria-hidden="true"]')).toBeNull();
  expect(
    container
      .querySelector(".background-activity-disclosure")
      ?.hasAttribute("inert"),
  ).toBe(true);
});

it("catches up when visible again and clears timers on unmount", () => {
  render();
  advance(1_000);
  render({ visible: false });
  advance(60_000);
  expect(elapsed()).toBe("0:01");
  render();
  expect(elapsed()).toBe("1:01");
  act(() => root.render(null));
  expect(vi.getTimerCount()).toBe(0);
});
