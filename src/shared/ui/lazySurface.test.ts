// @vitest-environment happy-dom
import {
  act,
  Component,
  createElement,
  startTransition,
  Suspense,
  useState,
  type ReactNode,
} from "react";
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

it("renders a prefetched surface synchronously on its first mount", async () => {
  const Surface = lazySurface(async () => ({
    default: () => createElement("main", null, "Ready content"),
  }));
  await Surface.preload();
  act(() => root.render(createElement(Surface)));
  expect(container.textContent).toBe("Ready content");
});

it("retains a cold surface's state after loading and parent updates", async () => {
  function Counter({ label }: { label: string }) {
    const [count, setCount] = useState(0);
    return createElement(
      "button",
      { onClick: () => setCount((current) => current + 1) },
      `${label}: ${count}`,
    );
  }
  const Surface = lazySurface(async () => ({ default: Counter }));
  await act(async () =>
    root.render(createElement(Surface, { label: "First" })),
  );
  act(() => container.querySelector("button")!.click());
  act(() => root.render(createElement(Surface, { label: "Second" })));
  expect(container.textContent).toBe("Second: 1");
});

it("shares a cold import and publishes readiness to every mounted surface", async () => {
  const Component = ({ label }: { label: string }) =>
    createElement("main", null, label);
  let finish!: (module: { default: typeof Component }) => void;
  const pending = new Promise<{ default: typeof Component }>((resolve) => {
    finish = resolve;
  });
  const load = vi.fn(() => pending);
  const Surface = lazySurface(load);
  const render = (label: string) =>
    root.render(
      createElement(
        "div",
        null,
        createElement(Surface, { label }),
        createElement(Surface, { label: "Other" }),
      ),
    );
  act(() => render("First"));
  act(() => render("Latest"));
  expect(load).toHaveBeenCalledOnce();
  expect(container.querySelectorAll("main")).toHaveLength(0);
  await act(async () => finish({ default: Component }));
  expect(container.textContent).toBe("LatestOther");
});

it("reports a cold import failure to the surrounding error boundary", async () => {
  class Boundary extends Component<
    { children?: ReactNode },
    { failed: boolean }
  > {
    state = { failed: false };
    static getDerivedStateFromError() {
      return { failed: true };
    }
    render() {
      return this.state.failed ? "Load failed" : this.props.children;
    }
  }
  const logged = vi.spyOn(console, "error").mockImplementation(() => {});
  const Surface = lazySurface(async () => {
    throw new Error("Cold import failed");
  });
  try {
    await act(async () =>
      root.render(createElement(Boundary, null, createElement(Surface))),
    );
    expect(container.textContent).toBe("Load failed");
  } finally {
    logged.mockRestore();
  }
});
