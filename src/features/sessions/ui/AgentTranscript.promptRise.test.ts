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

const first: Block[] = [{ id: "u1", role: "user", text: "Hello" }];
const second: Block[] = [
  ...first,
  { id: "a1", role: "assistant", text: "Hi" },
  { id: "u2", role: "user", text: "Next" },
];

function render(blocks: Block[], busy = true) {
  act(() =>
    root.render(
      createElement(AgentTranscript, { blocks, busy, visible: true }),
    ),
  );
}

beforeEach(() => {
  appearance.layout = "chat";
  appearance.anchor = true;
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
  it("rises the first prompt of a fresh session from the upper screen", () => {
    render(first);

    expect(risenPrompts()).toEqual(["u1"]);
    const [rise] = animate.mock.calls[0];
    const [fade] = animate.mock.calls[1];
    expect(rise[0]).toEqual({ transform: "translateY(240px)" });
    expect(fade).toEqual([{ opacity: 0 }, { opacity: 1 }]);
  });

  it("holds the rest of the turn until the prompt settles", () => {
    vi.useFakeTimers();
    render(first);
    const turn = container.querySelector<HTMLElement>(".transcript-turn")!;
    expect(turn.dataset.promptRise).toBe("rising");

    const animation = animate.mock.results[0].value as { onfinish: () => void };
    act(() => animation.onfinish());
    expect(turn.dataset.promptRise).toBe("revealing");

    act(() => vi.advanceTimersByTime(320));
    expect(turn.dataset.promptRise).toBeUndefined();
    vi.useRealTimers();
  });

  it("rises each newly sent prompt", () => {
    render(first, false);
    render(second);

    expect(risenPrompts()).toEqual(["u2"]);
  });

  it("does not replay an existing conversation on mount", () => {
    render(second);

    expect(animate).not.toHaveBeenCalled();
  });

  it("stays still in the full-width layout", () => {
    appearance.layout = "full";
    render(first);
    render(second);

    expect(animate).not.toHaveBeenCalled();
  });

  it("stays still when prompts are not anchored to the top", () => {
    appearance.anchor = false;
    render(first);
    render(second);

    expect(animate).not.toHaveBeenCalled();
  });
});
