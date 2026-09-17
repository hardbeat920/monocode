// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ErrorBoundary } from "./ErrorBoundary";

function Boom(): never {
  throw new Error("test crash");
}

describe("ErrorBoundary", () => {
  let root: Root;
  let container: HTMLDivElement;

  beforeEach(() => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    container.remove();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("renders children when nothing throws", async () => {
    await act(async () => {
      root.render(
        createElement(
          ErrorBoundary,
          { label: "TestArea" },
          createElement("span", null, "healthy"),
        ),
      );
    });
    expect(container.textContent).toContain("healthy");
  });

  it("shows a retryable fallback instead of crashing the tree", async () => {
    const onError = vi.fn();
    const consoleSpy = vi
      .spyOn(console, "error")
      .mockImplementation(() => {});
    await act(async () => {
      root.render(
        createElement(
          ErrorBoundary,
          { label: "TestArea", onError },
          createElement(Boom),
        ),
      );
    });
    expect(container.textContent).toContain("Something went wrong");
    expect(container.textContent).toContain("TestArea");
    expect(container.textContent).toContain("test crash");
    expect(onError).toHaveBeenCalledTimes(1);

    const retry = container.querySelector("button");
    expect(retry?.textContent).toBe("Retry");
    await act(async () => {
      retry?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });
    consoleSpy.mockRestore();
  });
});
