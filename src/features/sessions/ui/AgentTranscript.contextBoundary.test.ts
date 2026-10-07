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

function render(kept: ContextKept, summary?: string) {
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
        ...(summary ? { summary } : {}),
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

it("keeps the harness's summary folded under the divider until asked", () => {
  render("recent", "**Earlier**: the user fixed auth.");
  const toggle = container.querySelector<HTMLButtonElement>(
    "[data-context-boundary] button",
  )!;
  expect(toggle.textContent).toBe("Show summary");
  expect(toggle.getAttribute("aria-expanded")).toBe("false");
  expect(container.textContent).not.toContain("the user fixed auth");

  act(() => toggle.click());
  expect(toggle.getAttribute("aria-expanded")).toBe("true");
  expect(toggle.textContent).toBe("Hide summary");
  expect(
    container.querySelector(
      '[data-context-boundary] [data-streamdown="strong"]',
    )?.textContent,
  ).toBe("Earlier");
});

it("offers no summary toggle when the harness shared none", () => {
  render("recent");
  expect(container.querySelector("[data-context-boundary] button")).toBeNull();
});

it("names a Mono's fresh session and what it carried", () => {
  const blocks: Block[] = [
    { id: "u1", role: "user", text: "First prompt" },
    { id: "a1", role: "assistant", text: "First answer" },
    {
      id: "r1",
      role: "system",
      text: "Fresh session started",
      contextBoundary: {
        kind: "rotation",
        trigger: "auto",
        reason: "idle",
        at: 1,
        kept: "recent",
        keptFromBlockId: "u1",
      },
    },
    { id: "u2", role: "user", text: "Second prompt" },
  ];
  act(() => root.render(createElement(AgentTranscript, { blocks })));
  const divider = container.querySelector('[role="separator"]');
  expect(divider?.textContent).toContain(
    "Fresh session started · after a break",
  );
  expect(divider?.textContent).toContain(
    "Earlier exchanges carried as one line each; the latest word for word",
  );
  expect(divider?.textContent).not.toContain("automatic");
});

it("hands a message the agent may have lost back to the composer", () => {
  const onAddToChat = vi.fn();
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
        kept: "user-messages",
      },
    },
    { id: "u2", role: "user", text: "Second prompt" },
  ];
  act(() =>
    root.render(createElement(AgentTranscript, { blocks, onAddToChat })),
  );
  const buttons = [
    ...container.querySelectorAll<HTMLButtonElement>(
      'button[aria-label="Add to chat"]',
    ),
  ];
  // Codex kept your prompts, so only its reply is offered.
  expect(buttons).toHaveLength(1);
  act(() => buttons[0].click());
  expect(onAddToChat).toHaveBeenCalledWith("First answer");
});

it("offers nothing to add without somewhere to add it", () => {
  render("none");
  expect(
    container.querySelector('button[aria-label="Add to chat"]'),
  ).toBeNull();
});
