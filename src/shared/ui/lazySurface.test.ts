// @vitest-environment happy-dom
import { act, createElement, startTransition, Suspense, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { lazySurface } from "./lazySurface";

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
});

function deferredSurface(label: string) {
  const component = () => createElement("main", null, label);
  let resolve!: (module: { default: typeof component }) => void;
  const pending = new Promise<{ default: typeof component }>((done) => {
    resolve = done;
  });
  const load = vi.fn(() => pending);
  const Surface = lazySurface(load, { suspense: false });
  return { Surface, load, finish: () => resolve({ default: component }) };
}

it("keeps the workspace visible during the first navigation and switches directly to the page", async () => {
  const inbox = deferredSurface("Inbox content");
  function Workspace() {
    const [open, setOpen] = useState(false);
    return createElement(
      "div",
      null,
      createElement(
        "button",
        {
          onClick: () => startTransition(() => setOpen(true)),
        },
        "Inbox",
      ),
      createElement("main", { hidden: open }, "Workspace content"),
      open ? createElement(inbox.Surface) : null,
    );
  }
  await act(async () =>
    root.render(
      createElement(
        Suspense,
        { fallback: "Blank loading frame" },
        createElement(Workspace),
      ),
    ),
  );
  await act(async () => container.querySelector("button")!.click());
  expect(inbox.load).toHaveBeenCalledOnce();
  expect(container.querySelector("main")!.hidden).toBe(false);
  expect(container.textContent).not.toContain("Blank loading frame");

  await act(async () => inbox.finish());
  expect(container.querySelector("main")!.hidden).toBe(true);
  expect(container.textContent).toContain("Inbox content");
  expect(container.textContent).not.toContain("Blank loading frame");
});

it("uses the most recent destination when another page is clicked before the first finishes", async () => {
  const inbox = deferredSurface("Inbox content");
  const notes = deferredSurface("Notes content");
  function Workspace() {
    const [page, setPage] = useState("workspace");
    return createElement(
      "div",
      null,
      ...["inbox", "notes"].map((destination) =>
        createElement(
          "button",
          {
            key: destination,
            onClick: () => startTransition(() => setPage(destination)),
          },
          destination,
        ),
      ),
      createElement(
        "main",
        { hidden: page !== "workspace" },
        "Workspace content",
      ),
      page === "inbox" ? createElement(inbox.Surface) : null,
      page === "notes" ? createElement(notes.Surface) : null,
    );
  }
  await act(async () =>
    root.render(
      createElement(
        Suspense,
        { fallback: "Blank loading frame" },
        createElement(Workspace),
      ),
    ),
  );
  const buttons = container.querySelectorAll("button");
  await act(async () => buttons[0]!.click());
  await act(async () => buttons[1]!.click());
  await act(async () => inbox.finish());
  expect(container.querySelector("main")!.hidden).toBe(false);
  expect(container.textContent).not.toContain("Inbox content");

  await act(async () => notes.finish());
  expect(container.textContent).toContain("Notes content");
  expect(container.textContent).not.toContain("Inbox content");
  expect(container.textContent).not.toContain("Blank loading frame");
});

it("shares one import between warmup and the first render", async () => {
  const inbox = deferredSurface("Inbox content");
  expect(inbox.load).not.toHaveBeenCalled();
  const first = inbox.Surface.preload();
  const second = inbox.Surface.preload();
  expect(first).toBe(second);
  inbox.finish();
  await first;
  await act(async () =>
    root.render(
      createElement(Suspense, { fallback: null }, createElement(inbox.Surface)),
    ),
  );
  expect(inbox.load).toHaveBeenCalledOnce();
  expect(container.textContent).toBe("Inbox content");
});

it("retries a failed warmup when the page is opened", async () => {
  const component = () => createElement("main", null, "Inbox content");
  const load = vi
    .fn()
    .mockRejectedValueOnce(new Error("Warmup failed"))
    .mockResolvedValue({ default: component });
  const Surface = lazySurface(load);
  await expect(Surface.preload()).rejects.toThrow("Warmup failed");
  await act(async () => root.render(createElement(Surface)));
  expect(load).toHaveBeenCalledTimes(2);
  expect(container.textContent).toBe("Inbox content");
});
