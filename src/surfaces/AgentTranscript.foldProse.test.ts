// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Block } from "../lib/session";
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

describe("prose folded into the work trail", () => {
  it("marks a mid-turn note as process, leaving the answer at full strength", () => {
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

    // The trail stays collapsed until asked for, so none of what it holds —
    // the note included — is in the DOM yet.
    expect(container.querySelector(".zen-fold-prose")).toBeNull();
    const toggle = container.querySelector<HTMLButtonElement>(
      'button[aria-label="Show the work"]',
    )!;
    expect(toggle).not.toBeNull();

    act(() => toggle.click());

    // Only the mid-turn note carries the marker; the tool rows it sits
    // between stay ordinary rail entries.
    const prose = container.querySelectorAll(".zen-fold-prose");
    expect(prose).toHaveLength(1);
    expect(prose[0].textContent).toContain("Trying the other config.");
    expect(prose[0].textContent).not.toContain(
      "The investigation is complete.",
    );

    // The answer renders the same markdown root, outside the demoted wrapper.
    const answer = Array.from(
      container.querySelectorAll(".agent-markdown"),
    ).find((el) => el.textContent?.includes("The investigation is complete."));
    expect(answer).not.toBeUndefined();
    expect(answer?.closest(".zen-fold-prose")).toBeNull();
  });
});
