// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Block } from "../lib/session";
import { AgentTranscript } from "./AgentTranscript";

let container: HTMLDivElement;
let root: Root;
let observers: Array<{ targets: Element[]; resize: () => void }>;

beforeEach(() => {
  observers = [];
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal(
    "ResizeObserver",
    class {
      targets: Element[] = [];
      constructor(readonly resize: () => void) {
        observers.push(this);
      }
      observe(target: Element) {
        this.targets.push(target);
      }
      disconnect() {
        this.targets = [];
      }
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

describe("subagent scrolling", () => {
  it("follows growing content to the bottom with only one live scroll window", () => {
    const blocks: Block[] = [
      { id: "user", role: "user", text: "Investigate auth" },
      {
        id: "spawn",
        role: "tool",
        text: "Auth review",
        tool: { callId: "spawn", kind: "agent", status: "in_progress" },
        agentRun: {
          name: "Auth review",
          steps: [
            { id: "intro", kind: "message", text: "Inspecting files." },
            ...Array.from({ length: 8 }, (_, index) => ({
              id: `step-${index}`,
              kind: "tool" as const,
              text: `Read file-${index}.ts`,
              toolKind: "read",
              status: "completed",
            })),
            {
              id: "last",
              kind: "tool",
              text: "Run check",
              toolKind: "shell",
              status: "failed",
              preview: {
                kind: "read",
                output: "Last line of output",
                contentOnly: true,
              },
            },
          ],
        },
      },
    ];
    act(() =>
      root.render(createElement(AgentTranscript, { blocks, busy: true })),
    );
    const button = container.querySelector<HTMLButtonElement>(
      'button[aria-label="Show Auth review\'s work"]',
    );
    expect(button).not.toBeNull();
    act(() => button!.click());
    const scroller =
      container.querySelector<HTMLDivElement>(".zen-phase-live")!;
    expect(scroller).not.toBeNull();
    expect(scroller.parentElement?.closest(".zen-phase-live")).toBeNull();
    let height = 600;
    let top = 0;
    Object.defineProperties(scroller, {
      scrollHeight: { get: () => height },
      clientHeight: { get: () => 280 },
      scrollTop: {
        get: () => top,
        set: (value: number) => {
          top = Math.max(0, Math.min(value, height - 280));
        },
      },
    });
    const observer = observers.find((item) =>
      item.targets.includes(scroller.firstElementChild!),
    )!;
    expect(observer).toBeDefined();
    act(() => observer.resize());
    expect(top).toBe(320);
    // A row expansion or a markdown layout change resizes this inner body.
    height = 900;
    act(() => observer.resize());
    expect(top).toBe(620);
    // Reading older work must pause automatic following.
    act(() =>
      scroller.dispatchEvent(new WheelEvent("wheel", { deltaY: -100 })),
    );
    top = 300;
    height = 1000;
    act(() => observer.resize());
    expect(top).toBe(300);
  });
});
