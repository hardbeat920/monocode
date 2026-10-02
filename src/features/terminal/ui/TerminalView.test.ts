// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import type { Terminal as HeadlessTerminal } from "@xterm/headless";
import { TerminalView } from "./TerminalView";

const { terms, listeners } = vi.hoisted(() => ({
  terms: [] as HeadlessTerminal[],
  listeners: new Map<string, (event: { payload: unknown }) => void>(),
}));

// The real parser and modes, without a renderer happy-dom cannot host.
vi.mock("@xterm/xterm", async () => {
  const { Terminal: Headless } = await import("@xterm/headless");
  class Terminal extends Headless {
    element = undefined;
    constructor(options: Record<string, unknown>) {
      super({ cols: 80, rows: 24, allowProposedApi: true, scrollback: 100 });
      void options;
      terms.push(this);
    }
    open() {}
    focus() {}
    onRender() {
      return { dispose() {} };
    }
    attachCustomKeyEventHandler() {}
    attachCustomWheelEventHandler() {}
    paste(text: string) {
      const prepared = text.replace(/\r?\n/g, "\r");
      this.input(
        this.modes.bracketedPasteMode
          ? `\x1b[200~${prepared}\x1b[201~`
          : prepared,
        true,
      );
    }
  }
  return { Terminal };
});
vi.mock("@xterm/xterm/css/xterm.css", () => ({}));
vi.mock("../model/terminalLayout", () => ({
  applyTerminalChrome: vi.fn(),
  fitTerminal: vi.fn(() => null),
  resetGridStretch: vi.fn(),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (name: string, handler: (event: { payload: unknown }) => void) => {
    listeners.set(name, handler);
    return () => listeners.delete(name);
  }),
}));

type Attach = { reattached: boolean; restore: string };

let root: Root;
let container: HTMLDivElement;
let answerSpawn: (attach: Attach) => void;

function output(id: string, text: string) {
  const data = btoa(text);
  listeners.get("pty-data")?.({ payload: { id, data } });
}

function calls(command: string) {
  return vi
    .mocked(invoke)
    .mock.calls.filter(([name]) => name === command)
    .map(([, args]) => args);
}

async function settle() {
  const term = terms.at(-1)!;
  await act(async () => {
    await new Promise<void>((resolve) => term.write("", resolve));
  });
}

async function mount(id: string) {
  await act(async () => {
    root.render(createElement(TerminalView, { id, cwd: "/tmp", active: true }));
  });
  await settle();
  return terms.at(-1)!;
}

beforeEach(() => {
  terms.length = 0;
  listeners.clear();
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockImplementation(async (command) => {
    if (command === "pty_spawn") {
      return new Promise<Attach>((resolve) => (answerSpawn = resolve));
    }
    if (command === "pty_status") return { foreground: null };
    return undefined;
  });
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

afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

describe("a terminal view after a reload", () => {
  it("restores the running program's modes before it takes input", async () => {
    const term = await mount("term-1");
    expect(calls("pty_spawn")).toEqual([
      { id: "term-1", cwd: "/tmp", cols: 80, rows: 24 },
    ]);

    // Vim's redraw arrives before `pty_spawn` answers, and a key typed
    // now would be encoded for the wrong modes: neither goes through.
    output("term-1", "\x1b[H\x1b[2J~");
    term.paste("early\n");
    await settle();
    expect(calls("pty_write")).toEqual([]);
    expect(term.buffer.active.getLine(0)?.translateToString(true)).toBe("");

    await act(async () =>
      answerSpawn({
        reattached: true,
        restore: "\x1b[?1049h\x1b[?1h\x1b=\x1b[?2004h",
      }),
    );
    await settle();
    expect(term.buffer.active.type).toBe("alternate");
    expect(term.modes.applicationCursorKeysMode).toBe(true);
    expect(term.buffer.active.getLine(0)?.translateToString(true)).toBe("~");

    term.paste("echo one\necho two\n");
    await settle();
    expect(calls("pty_write")).toEqual([
      { id: "term-1", data: "\x1b[200~echo one\recho two\r\x1b[201~" },
    ]);
  });
});

describe("closing a terminal tab", () => {
  it("stops its process", async () => {
    await mount("term-2");
    await act(async () => answerSpawn({ reattached: false, restore: "" }));
    await act(async () => root.render(createElement("div")));
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
    expect(calls("pty_kill")).toEqual([{ id: "term-2" }]);
  });
});
