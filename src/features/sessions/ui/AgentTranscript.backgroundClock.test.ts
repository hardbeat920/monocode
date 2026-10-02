// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Block } from "../model/session";
import { AgentTranscript } from "./AgentTranscript";

let container: HTMLDivElement;
let root: Root;

// Claude yielded with a command still running, then the person replied. The
// reply is steered into the run in flight, so it has no clock of its own.
const startedAt = Date.UTC(2026, 8, 30, 10, 0, 0);
const blocks: Block[] = [
  { id: "prompt", role: "user", text: "start the dev server", startedAt },
  { id: "answer", role: "assistant", text: "It is running." },
  { id: "steer", role: "user", text: "is it up yet?" },
];

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(startedAt + 95_000);
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal("ResizeObserver", class {
    observe() {}
    disconnect() {}
  });
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  vi.useRealTimers();
  container.remove();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

function render(visible: boolean, transcript: Block[] = blocks) {
  act(() =>
    root.render(
      createElement(AgentTranscript, {
        blocks: transcript,
        busy: true,
        visible,
        backgroundTasks: ["npm run dev"],
      }),
    ),
  );
}

describe("AgentTranscript background clock", () => {
  it("keeps counting from the prompt that started the run", () => {
    render(true);
    expect(container.textContent).toContain("Working for 1m 35s");
    expect(container.textContent).toContain("running in background");
  });

  it("does not start over when the tab is shown again", () => {
    render(true);
    render(false);
    render(true);
    expect(container.textContent).toContain("Working for 1m 35s");
  });

  it("does not reach back past a turn that already ended", () => {
    // A send the host has not confirmed yet carries no clock, but the turn
    // before it finished long ago.
    render(true, [
      {
        id: "old",
        role: "user",
        text: "earlier",
        startedAt: startedAt - 3_600_000,
        durationMs: 5_000,
      },
      { id: "old-answer", role: "assistant", text: "Done." },
      { id: "pending", role: "user", text: "next prompt" },
    ]);
    expect(container.textContent).not.toContain("61m");
    expect(container.textContent).toContain("next promptWorking for 1s");
  });
});
