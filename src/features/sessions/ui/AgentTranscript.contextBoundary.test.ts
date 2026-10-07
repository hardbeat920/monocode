// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { Block, ContextKept } from "../model/session";
import { AgentTranscript } from "./AgentTranscript";

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal("matchMedia", () => ({ matches: true }));
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
  vi.unstubAllGlobals();
});

function render(kept: ContextKept) {
  const blocks: Block[] = [
    { id: "u1", role: "user", text: "First prompt" },
    { id: "a1", role: "assistant", text: "First answer" },
    {
      id: "b1",
      role: "system",
      text: "Context compacted",
      contextBoundary: {
        kind: "compaction",
        trigger: "auto",
        at: 1,
        kept,
        preTokens: 204_481,
        postTokens: 15_071,
      },
    },
    { id: "u2", role: "user", text: "Second prompt" },
  ];
  act(() => root.render(createElement(AgentTranscript, { blocks })));
}

function dimmed(text: string): boolean {
  const row = [
    ...container.querySelectorAll("[data-transcript-search-item]"),
  ].find((element) => element.textContent?.includes(text));
  return !!row?.classList.contains("opacity-55");
}

it("labels the boundary with its trigger, size and what was kept", () => {
  render("user-messages");
  const divider = container.querySelector('[role="separator"]');
  expect(divider?.textContent).toContain(
    "Context compacted · automatic · 204K → 15K tokens",
  );
  expect(divider?.textContent).toContain(
    "Your recent prompts were kept; earlier replies were summarized",
  );
});

it("dims what the agent no longer holds, and only that", () => {
  render("user-messages");
  expect(dimmed("First answer")).toBe(true);
  expect(dimmed("First prompt")).toBe(false);
  expect(dimmed("Second prompt")).toBe(false);

  render("none");
  expect(dimmed("First prompt")).toBe(true);

  render("recent");
  expect(dimmed("First answer")).toBe(false);
});
