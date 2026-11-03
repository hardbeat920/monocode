// @vitest-environment happy-dom
import { act, createElement, useSyncExternalStore } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import type { StreamdownProps } from "streamdown";

const renderer = vi.hoisted(() => ({
  blocks: ["Before.", "\n\n", "Unchanged."],
  listeners: new Set<() => void>(),
  presented: vi.fn(),
}));
vi.mock("../model/streamingPerformance", () => ({
  useStreamPresentation: () => renderer.presented,
}));
vi.mock("streamdown", async (original) => {
  const actual = await original<typeof import("streamdown")>();
  return {
    ...actual,
    // Model Streamdown retaining its old block list until a later transition.
    Streamdown({ BlockComponent = actual.Block }: StreamdownProps) {
      const blocks = useSyncExternalStore(
        (listener) => {
          renderer.listeners.add(listener);
          return () => renderer.listeners.delete(listener);
        },
        () => renderer.blocks,
      );
      return createElement(
        "div",
        null,
        blocks.map((content, index) =>
          createElement(BlockComponent, {
            key: index,
            content,
            index,
            isIncomplete: false,
            shouldNormalizeHtmlIndentation: true,
            shouldParseIncompleteMarkdown: false,
          }),
        ),
      );
    },
  };
});
import { AgentMarkdown } from "./AgentMarkdown";

it("waits for every rendered block, including an edit before an unchanged tail", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const text = "After.\n\nUnchanged.";
  try {
    await act(async () =>
      root.render(
        createElement(AgentMarkdown, {
          text,
          measurement: { pending: [] },
        }),
      ),
    );
    expect(host.textContent).toBe("Before.Unchanged.");
    expect(renderer.presented).not.toHaveBeenCalled();
    await act(async () => {
      renderer.blocks = ["After.", "\n\n", "Unchanged."];
      for (const listener of renderer.listeners) listener();
    });
    expect(host.textContent).toBe("After.Unchanged.");
    expect(renderer.presented).toHaveBeenCalledExactlyOnceWith(text.length);
  } finally {
    await act(async () => root.unmount());
    host.remove();
    vi.unstubAllGlobals();
  }
});
