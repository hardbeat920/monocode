// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { PanelStack } from "./PanelStack";

let root: Root;
let container: HTMLElement;
beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.useFakeTimers();
  container = document.createElement("div");
  root = createRoot(container);
});
afterEach(() => {
  act(() => root.unmount());
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

const render = (keys: string[]) =>
  act(() =>
    root.render(
      createElement(PanelStack, {
        pages: keys.map((key) => ({ key, node: createElement("p", null, key) })),
        children: createElement("p", null, "panel"),
      }),
    ),
  );
const reachable = () =>
  [...container.querySelectorAll("p")]
    .filter((p) => !p.closest("[inert]"))
    .map((p) => p.textContent);

it("stacks pages, with only the top one in reach, and lets them slide away", () => {
  render([]);
  expect(reachable()).toEqual(["panel"]);
  render(["settings"]);
  act(() => void vi.advanceTimersByTime(50));
  expect(reachable()).toEqual(["settings"]);
  render(["settings", "habit"]);
  act(() => void vi.advanceTimersByTime(50));
  expect(reachable()).toEqual(["habit"]);
  expect(container.textContent).toBe("panelsettingshabit");
  // Back: the habit page slides out, staying mounted until it is gone.
  render(["settings"]);
  expect(container.textContent).toBe("panelsettingshabit");
  expect(reachable()).toEqual(["settings"]);
  act(() => void vi.advanceTimersByTime(400));
  expect(container.textContent).toBe("panelsettings");
  render([]);
  act(() => void vi.advanceTimersByTime(400));
  expect(container.textContent).toBe("panel");
  expect(reachable()).toEqual(["panel"]);
});
