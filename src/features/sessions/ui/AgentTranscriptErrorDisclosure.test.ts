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

describe("MonoCode CLI disclosure", () => {
  it("shows a compact row without a disclosure for a successful call", () => {
    const command =
      "/repo/target/debug/MonoCode.app/Contents/MacOS/monocode app notes.list";
    const blocks: Block[] = [
      { id: "user", role: "user", text: "/monocode list notes" },
      {
        id: "notes",
        role: "tool",
        text: command,
        tool: {
          kind: "shell",
          status: "completed",
          detail: '{"ok":true,"result":{"notes":[{"title":"Ideas"}]}}',
        },
      },
    ];
    act(() =>
      root.render(createElement(AgentTranscript, { blocks, busy: true })),
    );

    const row = container.querySelector<HTMLElement>(
      '[data-monocode-tool-call="notes.list"]',
    );
    expect(row?.querySelector("button")).toBeNull();
    expect(row?.querySelector("pre")).toBeNull();
    expect(row?.textContent).toContain("Ranmonocode app notes.list");
    expect(row?.querySelector('img[src="/monocode.png"]')).not.toBeNull();
    expect(row?.querySelector(".bg-content\\/6")).not.toBeNull();
    expect(container.textContent).not.toContain("Contents/MacOS/monocode");
    expect(container.textContent).not.toContain('"title":"Ideas"');
  });

  it("reveals a failed call's error when clicked", () => {
    const blocks: Block[] = [
      { id: "user", role: "user", text: "/monocode list notes" },
      {
        id: "notes",
        role: "tool",
        text: "monocode app notes.list",
        tool: { kind: "shell", status: "failed", detail: "Connection refused" },
      },
    ];
    act(() =>
      root.render(createElement(AgentTranscript, { blocks, busy: true })),
    );

    const trigger = container.querySelector<HTMLButtonElement>(
      'button[aria-label="Show error details for MonoCode: List notes"]',
    );
    expect(trigger?.getAttribute("aria-expanded")).toBe("false");
    expect(container.textContent).not.toContain("Connection refused");

    act(() => trigger?.click());
    expect(trigger?.getAttribute("aria-expanded")).toBe("true");
    expect(container.textContent).toContain("Connection refused");
  });
});
