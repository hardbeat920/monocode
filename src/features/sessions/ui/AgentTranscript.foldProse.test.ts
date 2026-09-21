// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Block } from "../model/session";
import { AgentTranscript } from "./AgentTranscript";

let container: HTMLDivElement;
let root: Root;

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
  vi.unstubAllGlobals();
});

function tool(id: string): Block {
  return {
    id,
    role: "tool",
    text: `Inspect ${id}`,
    tool: { kind: "shell", status: "completed" },
  };
}

describe("prose inside the work span", () => {
  it("folds step narration with the work; delivered answers keep their row", () => {
    const blocks: Block[] = [
      { id: "user", role: "user", text: "Keep me posted" },
      tool("t1"),
      { id: "note", role: "assistant", text: "Trying the other config." },
      tool("t2"),
      {
        id: "answer",
        role: "assistant",
        text: "The investigation is complete.",
      },
    ];
    act(() => root.render(createElement(AgentTranscript, { blocks })));

    // Prose sandwiched by two finished work groups is narration: it folds
    // with the work. The answer after the last work group keeps its row.
    expect(container.textContent).not.toContain("Trying the other config.");
    expect(container.textContent).toContain("The investigation is complete.");
    expect(container.textContent).not.toContain("Inspect t1");
    expect(container.textContent).not.toContain("Inspect t2");
    const toggle = container.querySelector<HTMLButtonElement>(
      'button[aria-label="Show the work"]',
    )!;
    expect(toggle).not.toBeNull();

    act(() => toggle.click());

    // Expanded, the work and its narration return to their original order.
    const text = container.textContent ?? "";
    expect(text).toContain("Inspect t1");
    expect(text).toContain("Inspect t2");
    expect(text.indexOf("Inspect t1")).toBeLessThan(
      text.indexOf("Trying the other config."),
    );
    expect(text.indexOf("Trying the other config.")).toBeLessThan(
      text.indexOf("Inspect t2"),
    );
  });
});
