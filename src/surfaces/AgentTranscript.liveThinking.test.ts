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

const FIRST_THOUGHT = "Pulling the frames out of the recording.";
const SECOND_THOUGHT = "The boxes drop at the range the tracker resets.";

function tool(id: string): Block {
  return {
    id,
    role: "tool",
    text: `Inspect ${id}`,
    tool: { kind: "shell", status: "completed" },
  };
}

function turn(...thoughts: string[]): Block[] {
  return [
    { id: "user", role: "user", text: "Fix the camera detection" },
    {
      id: "preamble",
      role: "assistant",
      text: "Starting with the runbook, then the lab code.",
    },
    tool("t1"),
    ...thoughts.map((text, index) => ({
      id: `thought-${index}`,
      role: "reasoning" as const,
      text,
      streaming: index === thoughts.length - 1,
    })),
  ];
}

function collapseWork() {
  const toggle = container.querySelector<HTMLButtonElement>(
    'button[aria-label^="Hide the steps for"]',
  );
  expect(toggle).not.toBeNull();
  act(() => toggle!.click());
}

describe("a live group that was closed", () => {
  it("keeps reporting the agent's newest thought", () => {
    act(() =>
      root.render(
        createElement(AgentTranscript, {
          blocks: turn(FIRST_THOUGHT),
          busy: true,
        }),
      ),
    );
    expect(container.textContent).toContain(FIRST_THOUGHT);

    collapseWork();

    // The steps are folded away — and the line that says what the agent is on
    // is not, or a turn that narrates in reasoning reads as stalled.
    expect(
      container.querySelector('.zen-phase-body[data-open="false"]'),
    ).not.toBeNull();
    expect(container.textContent).toContain(FIRST_THOUGHT);
  });

  it("follows the thought as the agent moves on", () => {
    act(() =>
      root.render(
        createElement(AgentTranscript, {
          blocks: turn(FIRST_THOUGHT),
          busy: true,
        }),
      ),
    );
    collapseWork();
    act(() =>
      root.render(
        createElement(AgentTranscript, {
          blocks: turn(FIRST_THOUGHT, SECOND_THOUGHT),
          busy: true,
        }),
      ),
    );

    expect(container.textContent).toContain(SECOND_THOUGHT);
    expect(container.textContent).not.toContain(FIRST_THOUGHT);
  });

  it("says nothing more once the turn has settled", () => {
    act(() =>
      root.render(
        createElement(AgentTranscript, { blocks: turn(FIRST_THOUGHT) }),
      ),
    );

    expect(container.textContent).not.toContain(FIRST_THOUGHT);
  });
});
