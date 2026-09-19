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

const FIRST_THOUGHT = "Reading the runbook to find where detection is set.";
const SECOND_THOUGHT = "The tracker resets the boxes at 40 m.";

function tool(id: string, text: string): Block {
  return {
    id,
    role: "tool",
    text,
    tool: { kind: "shell", title: text, status: "completed" },
  };
}

function think(id: string, text: string, streaming = false): Block {
  return { id, role: "reasoning", text, streaming };
}

/** A harness that narrates in reasoning: one preamble, then only thoughts. */
function turn(...thoughts: Block[]): Block[] {
  return [
    { id: "u", role: "user", text: "Fix the camera detection" },
    { id: "p0", role: "assistant", text: "Starting with the runbook." },
    ...thoughts,
    tool("t1", "bash npm test"),
  ];
}

function header(): HTMLButtonElement {
  const button = container.querySelector<HTMLButtonElement>(
    'button[aria-label^="Hide the steps for"], button[aria-label^="Show the steps for"]',
  );
  expect(button).not.toBeNull();
  return button!;
}

describe("the header of a live group", () => {
  it("says what the agent is thinking, not how many files it read", () => {
    act(() =>
      root.render(
        createElement(AgentTranscript, {
          blocks: turn(think("r1", FIRST_THOUGHT, true)),
          busy: true,
        }),
      ),
    );

    expect(header().textContent).toBe(FIRST_THOUGHT);
    // The header holds the line; the trail under it does not repeat it.
    expect(container.textContent?.split(FIRST_THOUGHT)).toHaveLength(2);
  });

  it("follows the agent as it moves on", () => {
    act(() =>
      root.render(
        createElement(AgentTranscript, {
          blocks: turn(think("r1", FIRST_THOUGHT, true)),
          busy: true,
        }),
      ),
    );
    act(() =>
      root.render(
        createElement(AgentTranscript, {
          blocks: turn(
            think("r1", FIRST_THOUGHT),
            think("r2", SECOND_THOUGHT, true),
          ),
          busy: true,
        }),
      ),
    );

    expect(header().textContent).toBe(SECOND_THOUGHT);
    // The thought the agent moved on from drops into the trail.
    expect(container.textContent).toContain(FIRST_THOUGHT);
  });

  it("counts the calls until the agent has thought anything", () => {
    act(() =>
      root.render(
        createElement(AgentTranscript, {
          blocks: [
            { id: "u", role: "user", text: "Fix the camera detection" },
            tool("t1", "bash npm test"),
            tool("t2", "bash npm run lint"),
          ],
          busy: true,
        }),
      ),
    );

    expect(header().textContent).toBe("Running 2 commands");
  });

  it("leaves a thought the header cannot show as a step", () => {
    act(() =>
      root.render(
        createElement(AgentTranscript, {
          blocks: turn(think("r1", "```sh\nnpm test -- detect\n```", true)),
          busy: true,
        }),
      ),
    );

    expect(header().textContent).toBe("Running a command");
    expect(
      container.querySelector('button[aria-label^="Show thinking:"]'),
    ).not.toBeNull();
  });

  it("keeps reasoning off a group the reader closed", () => {
    act(() =>
      root.render(
        createElement(AgentTranscript, {
          blocks: turn(think("r1", FIRST_THOUGHT, true)),
          busy: true,
        }),
      ),
    );
    act(() => header().click());

    expect(header().textContent).toBe("Running a command");
    expect(container.textContent).not.toContain(FIRST_THOUGHT);
  });

  it("keeps reasoning out of a turn that has settled", () => {
    act(() =>
      root.render(
        createElement(AgentTranscript, {
          blocks: [
            ...turn(think("r1", FIRST_THOUGHT)),
            { id: "p1", role: "assistant", text: "Done: the threshold moved." },
          ],
        }),
      ),
    );

    expect(container.textContent).not.toContain(FIRST_THOUGHT);
  });
});
