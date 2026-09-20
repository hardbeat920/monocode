// @vitest-environment happy-dom
import { act, createElement, StrictMode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { BrowserPane, useBrowserOpen } from "./BrowserPane";

const native = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: native.invoke }));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ listen: native.listen }),
}));
function Harness() {
  const [open, setOpen] = useBrowserOpen();
  return createElement(
    "div",
    { style: { display: "flex" } },
    createElement("div", { "data-workspace": true }, "Chat"),
    createElement("button", { onClick: () => setOpen(!open) }, "Browser"),
    createElement(BrowserPane, { open }),
  );
}
let root: Root;
let container: HTMLDivElement;

beforeEach(async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  native.invoke.mockReset().mockResolvedValue(null);
  native.listen.mockResolvedValue(() => {});
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
  await act(async () =>
    root.render(createElement(StrictMode, null, createElement(Harness))),
  );
});

afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

async function click(text: string) {
  const button = [...document.querySelectorAll("button")].find(
    (button) =>
      button.textContent === text || button.getAttribute("aria-label") === text,
  )!;
  expect(button).toBeDefined();
  await act(async () => button.click());
}

it("opens a native child inside the panel and closes it without creating a window", async () => {
  await click("Browser");
  const form = document.querySelector("section[aria-label=Browser] form")!;
  await act(async () => {
    form.dispatchEvent(
      new Event("submit", { bubbles: true, cancelable: true }),
    );
  });
  expect(native.invoke).toHaveBeenCalledWith("embedded_browser", {
    request: expect.objectContaining({
      action: "open",
      url: "http://localhost:3000/",
    }),
  });
  await click("Back");
  expect(native.invoke).toHaveBeenCalledWith("embedded_browser", {
    request: { action: "back" },
  });
  expect(document.querySelector('[aria-label="Close browser"]')).toBeNull();
  await click("Browser");
  expect(native.invoke).toHaveBeenLastCalledWith("embedded_browser", {
    request: { action: "close" },
  });
  expect(document.querySelector("section[aria-label=Browser]")).toBeNull();
  expect(
    native.invoke.mock.calls.every(
      ([command]) => command === "embedded_browser",
    ),
  ).toBe(true);
});

it("displays a native creation error and keeps the panel usable", async () => {
  await click("Browser");
  native.invoke.mockRejectedValueOnce(new Error("Webview unavailable"));
  await act(async () => {
    document
      .querySelector("form")!
      .dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
  });
  expect(document.querySelector('[role="alert"]')?.textContent).toContain(
    "Webview unavailable",
  );
  expect(
    document.querySelector<HTMLButtonElement>('button[type="submit"]')
      ?.disabled,
  ).toBe(false);
});

it("uses only the top toggle and leaves no browser rail when closed", async () => {
  await click("Browser");
  expect(
    container.querySelector('[aria-label="Resize browser split"]'),
  ).not.toBeNull();
  expect(container.querySelector('[aria-label="Collapse browser"]')).toBeNull();
  expect(container.querySelector('[aria-label="Expand browser"]')).toBeNull();
  await click("Browser");
  expect(container.querySelector('section[aria-label="Browser"]')).toBeNull();
  expect(container.querySelector('[role="separator"]')).toBeNull();
  expect(container.children[0].children.length).toBe(2);
  await click("Browser");
  expect(
    container.querySelector('section[aria-label="Browser"]'),
  ).not.toBeNull();
});
