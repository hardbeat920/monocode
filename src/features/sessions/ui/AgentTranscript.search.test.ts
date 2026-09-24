// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { Block } from "../model/session";
import { AgentTranscript } from "./AgentTranscript";

let container: HTMLDivElement;
let root: Root;
let navigate: ((blockId: string | null) => boolean) | undefined;

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      disconnect() {}
    },
  );
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  navigate = undefined;
  vi.unstubAllGlobals();
});

it("reveals and marks a matching message in an older, unrendered turn", () => {
  const blocks: Block[] = Array.from({ length: 25 }, (_, index) => [
    { id: `user-${index}`, role: "user" as const, text: `Prompt ${index}` },
    {
      id: `answer-${index}`,
      role: "assistant" as const,
      text: `Answer ${index}`,
    },
  ]).flat();
  act(() =>
    root.render(
      createElement(AgentTranscript, {
        blocks,
        onNavigateReady: (callback) => {
          navigate = callback;
        },
      }),
    ),
  );
  expect(container.querySelector('[data-transcript-turn="user-0"]')).toBeNull();

  act(() => {
    expect(navigate?.("answer-0")).toBe(true);
  });
  expect(
    container.querySelector('[data-transcript-turn="user-0"]'),
  ).not.toBeNull();
  expect(
    container.querySelector('[data-transcript-search-current="true"]')
      ?.textContent,
  ).toContain("Answer 0");

  act(() => {
    navigate?.(null);
  });
  expect(
    container.querySelector('[data-transcript-search-current="true"]'),
  ).toBeNull();
});

it("opens folded work when its tool result is selected", () => {
  const blocks: Block[] = [
    { id: "user", role: "user", text: "Check the repository" },
    {
      id: "tool",
      role: "tool",
      text: "Inspect the search module",
      tool: { kind: "shell", status: "completed" },
    },
    { id: "answer", role: "assistant", text: "Done." },
  ];
  act(() =>
    root.render(
      createElement(AgentTranscript, {
        blocks,
        onNavigateReady: (callback) => {
          navigate = callback;
        },
      }),
    ),
  );

  act(() => {
    expect(navigate?.("tool")).toBe(true);
  });
  expect(
    container.querySelector('[data-transcript-search-current="true"]'),
  ).not.toBeNull();
  expect(
    container.querySelector('[data-transcript-search-current="true"]')
      ?.textContent,
  ).toContain("Inspect the search module");
});

it("paints matching words and clears them when find closes", () => {
  const registry = new Map<string, { ranges: Range[] }>();
  const escape = CSS.escape.bind(CSS);
  vi.stubGlobal("CSS", { escape, highlights: registry });
  vi.stubGlobal(
    "Highlight",
    class {
      ranges: Range[];
      constructor(...ranges: Range[]) {
        this.ranges = ranges;
      }
    },
  );
  act(() =>
    root.render(
      createElement(AgentTranscript, {
        blocks: [
          { id: "user", role: "user", text: "Hey can you check this?" },
          { id: "answer", role: "assistant", text: "Yes, I can check this." },
        ],
        onNavigateReady: (callback) => {
          navigate = callback;
        },
      }),
    ),
  );

  act(() => {
    navigate?.("user", "can you");
  });
  expect(
    registry
      .get("monocode-transcript-search-current")
      ?.ranges.map((range) => range.toString()),
  ).toEqual(["can you"]);
  expect(
    registry
      .get("monocode-transcript-search-match")
      ?.ranges.map((range) => range.toString()),
  ).toEqual(["can you"]);

  act(() => {
    navigate?.(null);
  });
  expect(registry.size).toBe(0);
});
