// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Block } from "../model/session";
import { saveRawToolCalls } from "../../settings/model/appearance";
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
  localStorage.clear();
  container.remove();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

const script = "python3 - <<'PY'\nimport sys\nprint(sys.argv)\nPY";

function render(blocks: Block[], busy = true, inlineWork = false) {
  act(() =>
    root.render(
      createElement(AgentTranscript, {
        blocks,
        busy,
        inlineWork,
        onApproval: () => {},
      }),
    ),
  );
}

function shellBlock(id: string, text: string, input: string): Block {
  return {
    id,
    role: "tool",
    text,
    tool: { kind: "execute", status: "completed", input },
  };
}

describe("tool call disclosure", () => {
  it("opens a multi-line command below its row and closes it again", () => {
    render([
      { id: "user", role: "user", text: "Run it" },
      shellBlock("py", "python3 - <<'PY'", script),
    ]);

    const trigger = container.querySelector<HTMLButtonElement>(
      'button[aria-label^="Show call details for"]',
    );
    expect(trigger?.getAttribute("aria-expanded")).toBe("false");
    expect(container.querySelector('pre[aria-label="Full tool call"]')).toBeNull();

    const row = container.querySelector<HTMLElement>(
      '[aria-label^="Tool call:"]',
    );
    const label = Array.from(row?.querySelectorAll("span") ?? []).find((el) =>
      el.textContent?.includes("python3"),
    );
    act(() => label?.click());
    expect(trigger?.getAttribute("aria-expanded")).toBe("true");
    const panel = container.querySelector<HTMLPreElement>(
      'pre[aria-label="Full tool call"]',
    );
    expect(panel?.textContent).toBe(script);

    act(() => trigger?.click());
    expect(container.querySelector('pre[aria-label="Full tool call"]')).toBeNull();
  });

  it("keeps a call that fits on its row quiet", () => {
    render([
      { id: "user", role: "user", text: "Status" },
      shellBlock("status", "git status", "git status"),
    ]);
    expect(
      container.querySelector('button[aria-label^="Show call details for"]'),
    ).toBeNull();
  });

  it("opens a one-line command once the row cuts it off", () => {
    vi.spyOn(HTMLElement.prototype, "scrollWidth", "get").mockReturnValue(900);
    vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(300);
    const command = `echo "${"x".repeat(240)}"`;
    render([
      { id: "user", role: "user", text: "Search" },
      shellBlock("long", command, command),
    ]);
    const trigger = container.querySelector<HTMLButtonElement>(
      'button[aria-label^="Show call details for"]',
    );
    expect(trigger).not.toBeNull();
    act(() => trigger?.click());
    expect(
      container.querySelector('pre[aria-label="Full tool call"]')?.textContent,
    ).toBe(command);
  });

  it("shows the call above the error on a failed row", () => {
    render([
      { id: "user", role: "user", text: "Run it" },
      {
        id: "py",
        role: "tool",
        text: "python3 - <<'PY'",
        tool: {
          kind: "execute",
          status: "failed",
          detail: "ModuleNotFoundError: No module named 'foo'",
          input: script,
        },
      },
    ]);
    const trigger = container.querySelector<HTMLButtonElement>(
      'button[aria-label^="Show error details for"]',
    );
    act(() => trigger?.click());
    const text = container.textContent ?? "";
    expect(text).toContain("print(sys.argv)");
    expect(text.indexOf("print(sys.argv)")).toBeLessThan(
      text.indexOf("ModuleNotFoundError"),
    );
  });

  it("opens a short failed command onto its error alone", () => {
    render([
      { id: "user", role: "user", text: "Status" },
      {
        id: "status",
        role: "tool",
        text: "git status",
        tool: {
          kind: "execute",
          status: "failed",
          detail: "fatal: not a git repository",
          input: "git status",
        },
      },
    ]);
    act(() =>
      container
        .querySelector<HTMLButtonElement>(
          'button[aria-label^="Show error details for"]',
        )
        ?.click(),
    );
    expect(container.textContent).toContain("fatal: not a git repository");
    expect(container.querySelector('pre[aria-label="Full tool call"]')).toBeNull();
  });

  it("lets a subagent's step open onto its call", () => {
    render([
      { id: "user", role: "user", text: "Review it" },
      {
        id: "agent",
        role: "tool",
        text: "Review",
        tool: { callId: "agent-1", kind: "agent", status: "in_progress" },
        agentRun: {
          name: "Review",
          steps: [
            {
              id: "s1",
              kind: "tool",
              text: "python3 - <<'PY'",
              toolKind: "execute",
              status: "completed",
              input: script,
            },
          ],
        },
      },
    ]);
    act(() =>
      container
        .querySelector<HTMLButtonElement>(
          'button[aria-label="Show Review\'s work"]',
        )
        ?.click(),
    );
    const trigger = container.querySelector<HTMLButtonElement>(
      'button[aria-label^="Show call details for"]',
    );
    expect(trigger).not.toBeNull();
    act(() => trigger?.click());
    expect(
      container.querySelector('pre[aria-label="Full tool call"]')?.textContent,
    ).toBe(script);
  });

  it("opens a read's arguments in a settled turn only with raw commands on", () => {
    const args = '{\n  "file_path": "/repo/src/a.ts",\n  "offset": 40,\n  "limit": 20\n}';
    render(
      [
        { id: "user", role: "user", text: "Look" },
        {
          id: "read",
          role: "tool",
          text: "Read src/a.ts",
          tool: {
            kind: "read",
            status: "completed",
            input: args,
            preview: { kind: "read", path: "/repo/src/a.ts", fileName: "a.ts" },
          },
        },
        { id: "done", role: "assistant", text: "Done." },
      ],
      false,
    );
    act(() =>
      container
        .querySelector<HTMLButtonElement>('button[aria-label="Show the work"]')
        ?.click(),
    );
    expect(
      container.querySelector('button[aria-label^="Show call details for"]'),
    ).toBeNull();
    act(() => saveRawToolCalls(true));
    const trigger = container.querySelector<HTMLButtonElement>(
      'button[aria-label^="Show call details for"]',
    );
    expect(trigger).not.toBeNull();
    act(() => trigger?.click());
    expect(
      container.querySelector('pre[aria-label="Full tool call"]')?.textContent,
    ).toBe(args);
  });

  it("keeps a readable row quiet, and opens it once raw commands are on", () => {
    render([
      { id: "user", role: "user", text: "List it" },
      shellBlock(
        "list",
        "ls -la /usr/bin | head -80",
        "ls -la /usr/bin | head -80",
      ),
    ]);
    expect(
      container.querySelector('button[aria-label^="Show call details for"]'),
    ).toBeNull();

    act(() => saveRawToolCalls(true));
    const trigger = container.querySelector<HTMLButtonElement>(
      'button[aria-label^="Show call details for"]',
    );
    expect(trigger).not.toBeNull();
    act(() => trigger?.click());
    expect(
      container.querySelector('pre[aria-label="Full tool call"]')?.textContent,
    ).toBe("ls -la /usr/bin | head -80");
  });

  it("opens a MonoCode CLI row onto its command only with raw commands on", () => {
    const command = `monocode app notes.write --json '{"body":"A long note body"}'`;
    render([
      { id: "user", role: "user", text: "Save it" },
      shellBlock("note", command, command),
    ]);
    expect(
      container.querySelector('[aria-label^="Show call details for MonoCode"]'),
    ).toBeNull();
    act(() => saveRawToolCalls(true));
    const trigger = container.querySelector<HTMLButtonElement>(
      '[aria-label^="Show call details for MonoCode"]',
    );
    expect(trigger).not.toBeNull();
    act(() => trigger?.click());
    expect(
      container.querySelector('pre[aria-label="Full tool call"]')?.textContent,
    ).toBe(command);
  });

  it("shows the command a Mono chat approval asks to run", () => {
    const script = "sed -n 1,10p src/app.ts\nsed -n 1,80p src/main.ts";
    render(
      [
        { id: "user", role: "user", text: "Read it" },
        {
          id: "sed",
          role: "tool",
          text: "Read src/main.ts",
          tool: { kind: "execute", status: "pending", input: script },
          approval: { requestId: 1 },
        },
      ],
      true,
      true,
    );
    expect(container.querySelector("[data-mono-work]")).not.toBeNull();
    const panel = container.querySelector<HTMLPreElement>(
      'pre[aria-label="Full tool call"]',
    );
    expect(panel?.textContent).toBe(script);
    const approve = Array.from(container.querySelectorAll("button")).find(
      (button) => /allow|approve/i.test(button.textContent ?? ""),
    );
    expect(approve).not.toBeUndefined();
    expect(
      panel!.compareDocumentPosition(approve!) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
  });

  it("shows the raw command above the buttons while it waits on approval", () => {
    render([
      { id: "user", role: "user", text: "Read it" },
      {
        id: "sed",
        role: "tool",
        text: "sed -n 1,80p src/main.ts",
        tool: {
          kind: "execute",
          status: "pending",
          input: "sed -n 1,80p src/main.ts",
        },
        approval: { requestId: 1 },
      },
    ]);
    const panel = container.querySelector<HTMLPreElement>(
      'pre[aria-label="Full tool call"]',
    );
    expect(panel?.textContent).toBe("sed -n 1,80p src/main.ts");
    expect(
      container.querySelector('button[aria-label^="Show call details for"]'),
    ).toBeNull();
    const approve = Array.from(container.querySelectorAll("button")).find(
      (button) => /allow|approve/i.test(button.textContent ?? ""),
    );
    expect(approve).not.toBeUndefined();
    expect(
      panel!.compareDocumentPosition(approve!) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
  });
});
