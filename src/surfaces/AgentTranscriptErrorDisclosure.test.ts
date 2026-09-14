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

describe("tool error disclosure", () => {
  it("keeps grouped error output collapsed until its failed row is clicked", () => {
    const blocks: Block[] = [
      { id: "user", role: "user", text: "Start the app" },
      {
        id: "command",
        role: "tool",
        text: "Run npm run dev",
        tool: {
          kind: "shell",
          status: "failed",
          detail: "Error: listen EPERM\n    at Server.setupListenHandle",
        },
      },
    ];

    act(() =>
      root.render(createElement(AgentTranscript, { blocks, busy: true })),
    );

    const trigger = container.querySelector<HTMLButtonElement>(
      'button[aria-label^="Show error details for"]',
    );
    expect(trigger).not.toBeNull();
    expect(trigger?.getAttribute("aria-expanded")).toBe("false");
    expect(container.textContent).not.toContain("Server.setupListenHandle");

    const failedRow = container.querySelector<HTMLElement>(
      '[aria-label^="Failed tool call:"]',
    );
    const message = Array.from(failedRow?.querySelectorAll("span") ?? []).find(
      (element) => element.textContent?.includes("npm run dev"),
    );
    expect(message).not.toBeNull();

    act(() => message?.click());
    expect(trigger?.getAttribute("aria-expanded")).toBe("true");
    expect(container.textContent).toContain("Server.setupListenHandle");

    act(() => trigger?.click());
    expect(trigger?.getAttribute("aria-expanded")).toBe("false");
    expect(container.textContent).not.toContain("Server.setupListenHandle");
  });
});
