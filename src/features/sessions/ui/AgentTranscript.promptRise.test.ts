// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Block } from "../model/session";
import { AgentTranscript } from "./AgentTranscript";

const appearance = vi.hoisted(() => ({
  layout: "chat" as "chat" | "full",
  anchor: true,
}));
vi.mock("../hooks/useTranscriptLayout", () => ({
  useTranscriptLayout: () => appearance.layout,
}));
vi.mock("../hooks/useTranscriptAnchor", () => ({
  useTranscriptAnchor: () => appearance.anchor,
}));

let container: HTMLDivElement;
let root: Root;
let animate: ReturnType<typeof vi.fn>;

let first: Block[];
let second: Block[];

function sendFirst(startedAt = Date.now()) {
  first = [{ id: "u1", role: "user", text: "Hello", startedAt }];
  second = [
    ...first,
    { id: "a1", role: "assistant", text: "Hi" },
    { id: "u2", role: "user", text: "Next" },
  ];
}

async function render(blocks: Block[], busy = true, visible = true) {
  await act(async () => {
    root.render(createElement(AgentTranscript, { blocks, busy, visible }));
  });
}

beforeEach(() => {
  appearance.layout = "chat";
  appearance.anchor = true;
  sendFirst();
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      disconnect() {}
    },
  );
  vi.stubGlobal("matchMedia", () => ({ matches: false }));
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
    function (this: HTMLElement) {
      return (
        this.dataset.promptAnchor
          ? { top: 0, height: 60 }
          : { top: 0, height: 800 }
      ) as DOMRect;
    },
  );
  animate = vi.fn(() => ({ cancel: vi.fn() }));
  HTMLElement.prototype.animate = animate as never;
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

function risenPrompts() {
  return [
    ...new Set(
      animate.mock.contexts.map(
        (element) => (element as HTMLElement).dataset.promptAnchor,
      ),
    ),
  ];
}

describe("prompt rise in the chat layout", () => {
  it("rises the first prompt of a fresh session from the upper screen", async () => {
    await render(first);

    expect(risenPrompts()).toEqual(["u1"]);
    const [rise] = animate.mock.calls[0];
    const [fade] = animate.mock.calls[1];
    expect(rise[0]).toEqual({ transform: "translateY(240px)" });
    expect(fade).toEqual([{ opacity: 0 }, { opacity: 1 }]);
  });

  it("holds the rest of the turn until the prompt settles", async () => {
    vi.useFakeTimers();
    await render(first);
    const turn = container.querySelector<HTMLElement>(".transcript-turn")!;
    expect(turn.dataset.promptRise).toBe("rising");

    const animation = animate.mock.results[0].value as { onfinish: () => void };
    act(() => animation.onfinish());
    expect(turn.dataset.promptRise).toBe("revealing");

    act(() => vi.advanceTimersByTime(320));
    expect(turn.dataset.promptRise).toBeUndefined();
    vi.useRealTimers();
  });

  it("rises each newly sent prompt", async () => {
    await render(first, false);
    await render(second);

    expect(risenPrompts()).toEqual(["u2"]);
  });

  it("does not replay an existing conversation on mount", async () => {
    await render(second);

    expect(animate).not.toHaveBeenCalled();
  });

  it("does not replay a first prompt sent a while ago when remounted mid-reply", async () => {
    sendFirst(Date.now() - 60_000);
    await render(first);

    expect(animate).not.toHaveBeenCalled();
  });

  it("does not rise a prompt sent while the tab was hidden when it returns", async () => {
    await render(first, false);
    await render(first, false, false);
    await render(second, true, false);
    await render(second);

    expect(animate).not.toHaveBeenCalled();
  });

  it("stays still in the full-width layout", async () => {
    appearance.layout = "full";
    await render(first);
    await render(second);

    expect(animate).not.toHaveBeenCalled();
  });

  it("stays still when prompts are not anchored to the top", async () => {
    appearance.anchor = false;
    await render(first);
    await render(second);

    expect(animate).not.toHaveBeenCalled();
  });
});
