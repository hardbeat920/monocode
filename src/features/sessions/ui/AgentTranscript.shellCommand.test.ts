// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { Block } from "../model/session";
import { AgentTranscript } from "./AgentTranscript";

vi.mock("../../settings/model/sounds", () => ({ playCue: vi.fn() }));

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

const turn: Block[] = [
  {
    id: "u",
    role: "user",
    text: "Fix it",
    startedAt: 1_000,
    durationMs: 5_000,
  },
  { id: "a", role: "assistant", text: "Fixed." },
];

function render(blocks: Block[], busy: boolean) {
  act(() =>
    root.render(
      createElement(AgentTranscript, {
        blocks,
        busy,
        harness: "claude",
        model: "sonnet",
        bottomAligned: true,
      }),
    ),
  );
}

it("shows a running `!` command on its own row, not as the agent working", () => {
  render(
    [
      ...turn,
      {
        id: "c1",
        role: "shell",
        text: "npm test",
        startedAt: 9_000,
        shell: { cwd: "/repo", status: "running" },
      },
    ],
    true,
  );
  const row = container.querySelector('[data-transcript-turn="c1"]');
  expect(row?.querySelector("[data-shell-command]")?.textContent).toMatch(
    /^!npm test.Running…$/,
  );
  expect(container.textContent).not.toMatch(/working/i);
});

it("shows what a finished command printed and how it exited", () => {
  render(
    [
      ...turn,
      {
        id: "c1",
        role: "shell",
        text: "npm test",
        shell: {
          cwd: "/repo",
          status: "done",
          stdout: "1 passed\n",
          stderr: "1 failed\n",
          exitCode: 1,
          truncated: true,
        },
      },
    ],
    false,
  );
  const row = container.querySelector("[data-shell-command]")!;
  expect(row.getAttribute("aria-label")).toBe(
    "Shell command you ran: npm test",
  );
  expect(row.querySelector("pre")?.textContent).toBe("1 passed\n1 failed");
  expect(row.textContent).toContain("Exit 1");
  expect(row.textContent).toContain("Output truncated");
});
