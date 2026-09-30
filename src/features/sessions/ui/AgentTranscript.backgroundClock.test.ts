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
  container.remove();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

// Claude yielded with a command still running, then the person replied. The
// reply is steered into the run in flight, so it has no clock of its own.
const startedAt = Date.now() - 95_000;
const blocks: Block[] = [
  { id: "prompt", role: "user", text: "start the dev server", startedAt },
  { id: "answer", role: "assistant", text: "It is running." },
  { id: "steer", role: "user", text: "is it up yet?" },
];

function render(visible: boolean) {
  act(() =>
    root.render(
      createElement(AgentTranscript, {
        blocks,
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
});
