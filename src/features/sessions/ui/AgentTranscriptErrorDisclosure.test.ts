// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Block } from "../model/session";
import { AgentTranscript, ToolCall } from "./AgentTranscript";

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
  it("keeps a successful call compact until its result is opened", () => {
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
          detailIsOutput: true,
        },
      },
    ];
    act(() =>
      root.render(createElement(AgentTranscript, { blocks, busy: true })),
    );

    const row = container.querySelector<HTMLElement>(
      '[data-monocode-tool-call="notes.list"]',
    );
    const trigger = row?.querySelector<HTMLButtonElement>('button[aria-expanded]');
    expect(trigger?.getAttribute("aria-expanded")).toBe("false");
    expect(row?.querySelector("pre")).toBeNull();
    expect(row?.textContent).toContain("Ranmonocode app notes.list");
    expect(row?.querySelector('img[src="/monocode.png"]')).not.toBeNull();
    expect(row?.querySelector(".bg-content\\/6")).not.toBeNull();
    expect(container.textContent).not.toContain("Contents/MacOS/monocode");
    expect(container.textContent).not.toContain('"title":"Ideas"');
    act(() => trigger?.click());
    expect(row?.querySelector("pre")?.textContent).toContain('  "ok": true');
    expect(row?.querySelector("pre")?.textContent).toContain('"title": "Ideas"');
    act(() => trigger?.click());
    expect(row?.querySelector("pre")).toBeNull();
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

describe("successful tool results", () => {
  it("uses shared, labelled search and read disclosures for completed OpenCode-style output, without leaking pending input", () => {
    const matches = "Found 47 matches\nD:\\primewire-desktop\\src\\main.ts\n  Line 7937: TODO fix";
    const blocks: Block[] = [
      { id: "u", role: "user", text: "Find the TODOs" },
      { id: "find", role: "tool", text: "Find TODO|FIXME|HACK|XXX", tool: {
        kind: "search", status: "completed", detailIsOutput: true,
        detail: matches, preview: { kind: "search", query: "TODO|FIXME|HACK|XXX" },
      } },
      { id: "read", role: "tool", text: "Read src/main.ts", tool: {
        kind: "read", status: "completed", detailIsOutput: true,
        detail: "export const count = 47;", preview: { kind: "read", path: "/repo/src/main.ts" },
      } },
      { id: "pending", role: "tool", text: "Find private input", tool: {
        kind: "search", status: "in_progress", detail: "input credentials", preview: { kind: "search", query: "private" },
      } },
    ];
    act(() => root.render(createElement(AgentTranscript, { blocks, busy: true, cwd: "/repo" })));
    const triggers = Array.from(container.querySelectorAll<HTMLButtonElement>('button[aria-label^="Show result for"]'));
    expect(triggers).toHaveLength(2);
    act(() => triggers[0].click());
    expect(container.querySelector("[data-tool-result-title]")?.textContent).toContain("Find TODO|FIXME|HACK|XXX");
    expect(container.querySelector("[data-tool-result-summary]")?.textContent).toBe("Found 47 matches");
    expect(container.querySelector("[data-tool-result-text]")?.textContent).toContain("Line 7937: TODO fix");
    expect(container.textContent).not.toContain("input credentials");
    act(() => triggers[1].click());
    expect(Array.from(container.querySelectorAll("[data-tool-result-title]")).map((node) => node.textContent)).toContain("Read src/main.ts");
  });

  it("exposes each available result inside a five-command activity group", () => {
    const blocks: Block[] = [
      { id: "u", role: "user", text: "Run five commands" },
      { id: "one", role: "tool", text: "Run first", tool: { kind: "shell", status: "completed", detailIsOutput: true, detail: '{"ok":true,"count":1}' } },
      { id: "two", role: "tool", text: "Run second", tool: { kind: "shell", status: "completed", preview: { kind: "shell", output: "second stdout\n" } } },
      { id: "three", role: "tool", text: "Run third", tool: { kind: "shell", status: "in_progress", detail: '{"input":"private args"}' } },
      { id: "four", role: "tool", text: "Run fourth", tool: { kind: "shell", status: "completed", detail: '{"input":"pending args"}' } },
      { id: "five", role: "tool", text: "Run fifth", tool: { kind: "shell", status: "success", detailIsOutput: true, detail: "fifth stdout" } },
    ];
    act(() => root.render(createElement(AgentTranscript, { blocks, busy: true })));
    expect(container.textContent).toContain("Running 5 commands");
    const triggers = Array.from(container.querySelectorAll<HTMLButtonElement>('button[aria-label^="Show result for"]'));
    expect(triggers).toHaveLength(3);
    expect(container.textContent).not.toContain("second stdout");
    expect(container.textContent).not.toContain("pending args");
    act(() => triggers[0].click());
    expect(triggers[0].getAttribute("aria-expanded")).toBe("true");
    expect(triggers[0].getAttribute("aria-controls")).toBeTruthy();
    expect(container.querySelectorAll("[data-tool-result]")).toHaveLength(1);
    expect(container.querySelector("[data-tool-result]")?.id).toBe(triggers[0].getAttribute("aria-controls"));
    expect(container.querySelector("[data-tool-result] pre")?.textContent).toContain('  "ok": true');
    act(() => triggers[1].click());
    expect(container.querySelectorAll("[data-tool-result]")).toHaveLength(2);
    expect(container.textContent).toContain("second stdout");
    expect(container.textContent).not.toContain("private args");
    expect(container.textContent).not.toContain("pending args");
    act(() => triggers[0].click());
    expect(container.querySelectorAll("[data-tool-result]")).toHaveLength(1);
  });

  it("discloses a successful read-card result with a keyboard-focusable toggle", () => {
    const block: Block = { id: "read", role: "tool", text: "Read src/file.ts", tool: {
      kind: "read", status: "completed", detailIsOutput: true, detail: '{"file":"src/file.ts","lines":["export const a = 1;"]}',
      preview: { kind: "read", path: "/repo/src/file.ts" },
    } };
    act(() => root.render(createElement(ToolCall, { block, cwd: "/repo" })));
    const trigger = container.querySelector<HTMLButtonElement>('button[aria-label^="Show result for"]');
    expect(trigger?.tabIndex).toBe(0);
    expect(trigger?.getAttribute("aria-expanded")).toBe("false");
    expect(container.querySelector("[data-tool-result]")).toBeNull();
    act(() => trigger?.click());
    expect(trigger?.getAttribute("aria-expanded")).toBe("true");
    expect(trigger?.getAttribute("aria-controls")).toBe(container.querySelector("[data-tool-result]")?.id);
    expect(container.querySelector("[data-tool-result] pre")?.textContent).toContain('  "file": "src/file.ts"');
  });

  it("opens grouped read and search results independently while retaining file links", () => {
    const blocks: Block[] = [
      { id: "u", role: "user", text: "Inspect" },
      { id: "read", role: "tool", text: "Read src/app.ts", tool: { kind: "read", status: "completed", detailIsOutput: true, detail: "file contents", preview: { kind: "read", path: "/repo/src/app.ts" } } },
      { id: "search", role: "tool", text: "Find token", tool: { kind: "search", status: "completed", preview: { kind: "search", query: "token", output: "search results" } } },
    ];
    const openFile = vi.fn();
    act(() => root.render(createElement(AgentTranscript, { blocks, busy: true, cwd: "/repo", onOpenFile: openFile })));
    const triggers = () => Array.from(container.querySelectorAll<HTMLButtonElement>('button[aria-label*=" result for "]'));
    expect(triggers()).toHaveLength(2);
    expect(container.querySelector("[data-tool-result]")).toBeNull();
    const file = container.querySelector<HTMLButtonElement>('button[title="src/app.ts"]');
    expect(file).not.toBeNull();
    act(() => file?.click());
    expect(openFile).toHaveBeenCalledWith("/repo/src/app.ts");
    expect(container.querySelector("[data-tool-result]")).toBeNull();
    act(() => triggers()[0].click());
    expect(container.textContent).toContain("file contents");
    expect(container.textContent).not.toContain("search results");
    act(() => triggers()[1].click());
    expect(container.querySelectorAll("[data-tool-result]")).toHaveLength(2);
    act(() => triggers()[0].click());
    expect(container.textContent).not.toContain("file contents");
    expect(container.textContent).toContain("search results");
  });

  it.each(["read", "search", "edit", "shell"])("exposes standalone %s results without suppressing its summary", (kind) => {
    const block: Block = { id: "t", role: "tool", text: kind === "edit" ? "Edit src/app.ts" : "Run tool", tool: { kind, status: "completed", detailIsOutput: true, detail: "<script>inert</script>\n{broken JSON…" } };
    act(() => root.render(createElement(ToolCall, { block })));
    const trigger = container.querySelector<HTMLButtonElement>('button[aria-label^="Show result for"]');
    expect(trigger).not.toBeNull();
    expect(container.querySelector("pre")).toBeNull();
    act(() => trigger?.click());
    expect(container.querySelector("[data-tool-result-text]")?.textContent).toBe(block.tool?.detail);
    expect(container.querySelector("script")).toBeNull();
  });

  it("does not disclose legacy ambiguous detail or an empty successful result", () => {
    const blocks: Block[] = [
      { id: "u", role: "user", text: "Inspect" },
      { id: "t", role: "tool", text: "Run tool", tool: { kind: "shell", status: "completed", detail: "pending input arguments" } },
    ];
    act(() => root.render(createElement(AgentTranscript, { blocks, busy: true })));
    expect(container.querySelector('button[aria-label*="result for"]')).toBeNull();
    expect(container.textContent).not.toContain("pending input arguments");
  });
});
