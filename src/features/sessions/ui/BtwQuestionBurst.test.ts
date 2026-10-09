// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { BtwQuestionBurst } from "./BtwQuestionBurst";

describe("BtwQuestionBurst", () => {
  let container: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    vi.useFakeTimers();
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);
  });

  afterEach(() => {
    act(() => root.unmount());
    container.remove();
    vi.unstubAllGlobals();
    vi.useRealTimers();
  });

  it("scatters question marks over the composer and ends itself", () => {
    const onDone = vi.fn();
    act(() =>
      root.render(
        createElement(BtwQuestionBurst, {
          rect: { left: 10, top: 20, width: 300, height: 80 },
          onDone,
        }),
      ),
    );

    const burst = container.querySelector<HTMLElement>(".btw-burst");
    expect(burst?.style.left).toBe("10px");
    expect(burst?.style.width).toBe("300px");
    expect(container.querySelectorAll(".btw-burst-mark").length).toBe(16);
    expect(container.textContent).toContain("?");

    act(() => vi.advanceTimersByTime(2000));
    expect(onDone).toHaveBeenCalledTimes(1);
  });
});
