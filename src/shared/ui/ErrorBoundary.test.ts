// @vitest-environment happy-dom
import { act, createElement, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { ErrorBoundary } from "./ErrorBoundary";

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
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

/** Throws on render, so the boundary has something to catch. */
function Bomb() {
  throw new Error("kaboom");
}

/** Throws a falsy value, which is legal but not an `Error`. */
function NullBomb() {
  throw null;
}

function renderBoundary(child: ReactNode) {
  return act(async () =>
    root.render(createElement(ErrorBoundary, null, child)),
  );
}

it("renders children while nothing has failed", async () => {
  await renderBoundary(createElement("main", null, "Healthy content"));
  expect(container.textContent).toContain("Healthy content");
  expect(container.querySelector("[role=alert]")).toBeNull();
});

it("shows a recoverable frame instead of unmounting the tree", async () => {
  const errorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
  await renderBoundary(createElement(Bomb, null));
  // The crash is the only trace of the failure, so it must be logged.
  expect(errorSpy).toHaveBeenCalled();
  const alert = container.querySelector("[role=alert]");
  expect(alert).not.toBeNull();
  expect(alert!.textContent).toContain("Something went wrong");
  expect(alert!.textContent).toContain("kaboom");
});

it("falls back for a thrown falsy value instead of retrying the child", async () => {
  vi.spyOn(console, "error").mockImplementation(() => {});
  // `throw null` is legal. Storing only the value would read as "no error"
  // and re-render the child forever.
  await renderBoundary(createElement(NullBomb, null));
  const alert = container.querySelector("[role=alert]");
  expect(alert).not.toBeNull();
  expect(alert!.textContent).toContain("Something went wrong");
});

it("reloads only when the user asks, never on its own", async () => {
  vi.spyOn(console, "error").mockImplementation(() => {});
  const reload = vi.fn();
  // `window.location.reload` is not writable, so replace the object.
  const originalLocation = window.location;
  Object.defineProperty(window, "location", {
    value: { ...originalLocation, reload },
    writable: true,
    configurable: true,
  });
  try {
    await renderBoundary(createElement(Bomb, null));
    // A deterministic crash must not loop: no reload yet.
    expect(reload).not.toHaveBeenCalled();

    await act(async () => container.querySelector("button")!.click());
    expect(reload).toHaveBeenCalledOnce();
  } finally {
    Object.defineProperty(window, "location", {
      value: originalLocation,
      writable: true,
      configurable: true,
    });
  }
});
