// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { createMono } from "../../features/monos/model/mono";
import { loadMonosEnabled } from "../../features/settings/model/settings";
import { MonoRailSection } from "./MonoRailSection";

let root: Root;
let container: HTMLElement;
beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const stored = new Map<string, string>();
  vi.stubGlobal("localStorage", {
    getItem: (key: string) => stored.get(key) ?? null,
    setItem: (key: string, value: string) => stored.set(key, value),
    removeItem: (key: string) => stored.delete(key),
  });
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

function render(props: Partial<Parameters<typeof MonoRailSection>[0]> = {}) {
  const onCreate = vi.fn();
  act(() =>
    root.render(
      createElement(MonoRailSection, {
        states: new Map(),
        onOpen: () => {},
        onCreate,
        onDelete: () => {},
        ...props,
      }),
    ),
  );
  return onCreate;
}

const intro = () => document.body.querySelector('[aria-label="Meet Monos"]');
const button = (label: string) =>
  [...document.body.querySelectorAll("button")].find(
    (entry) => entry.textContent === label,
  )!;

it("shows the intro card in place of the list while there is no mono", () => {
  const onCreate = render();
  expect(intro()).not.toBeNull();
  expect(container.querySelector('[aria-label="New mono"]')).toBeNull();

  act(() => button("Create mono").click());
  expect(onCreate).toHaveBeenCalledOnce();
});

it("turns monos off from the intro's switch", () => {
  render();
  act(() => container.querySelector<HTMLElement>('[role="switch"]')!.click());
  expect(loadMonosEnabled()).toBe(false);
});

it("lists monos once there are some, with the plus in the header", () => {
  createMono();
  render();
  expect(intro()).toBeNull();
  expect(container.querySelectorAll('[aria-label="New mono"]')).toHaveLength(1);
});

it("counts a mono's unread replies, except the one open", () => {
  const mono = createMono();
  const badge = () => container.querySelector("[data-mono-unread]");
  render({ unseenCounts: new Map([[mono.id, 3]]) });
  expect(badge()?.textContent).toBe("3");
  expect(
    container.querySelector(`[aria-label*="3 new replies"]`),
  ).not.toBeNull();

  render({ unseenCounts: new Map([[mono.id, 120]]) });
  expect(badge()?.textContent).toBe("99+");

  render({ activeId: mono.id, unseenCounts: new Map([[mono.id, 3]]) });
  expect(badge()).toBeNull();
});

const header = () =>
  container.querySelector<HTMLButtonElement>("button[aria-expanded]");

it("collapses the monos from the header, and remembers it", () => {
  createMono();
  render();
  expect(container.querySelectorAll("[data-mono-status]")).toHaveLength(1);

  act(() => header()?.click());
  expect(header()?.getAttribute("aria-expanded")).toBe("false");
  expect(container.querySelectorAll("[data-mono-status]")).toHaveLength(0);

  act(() => root.unmount());
  root = createRoot(container);
  render();
  expect(header()?.getAttribute("aria-expanded")).toBe("false");
  expect(container.querySelectorAll("[data-mono-status]")).toHaveLength(0);

  act(() => header()?.click());
  expect(container.querySelectorAll("[data-mono-status]")).toHaveLength(1);
});

it("expands the monos when one is added from the collapsed header", () => {
  createMono();
  const onCreate = render();
  act(() => header()?.click());
  act(() =>
    container
      .querySelector<HTMLButtonElement>('[aria-label="New mono"]')
      ?.click(),
  );
  expect(onCreate).toHaveBeenCalledOnce();
  expect(header()?.getAttribute("aria-expanded")).toBe("true");
});
