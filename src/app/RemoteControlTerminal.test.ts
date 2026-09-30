// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

/**
 * xterm is stubbed rather than run. What is under test is the wiring around it —
 * that the view opens on the buffer already decoded, reads through the fan-out
 * instead of subscribing, hands keystrokes to the pty, and never resizes it.
 * Rendering glyphs is xterm's business and needs a real layout engine.
 */
const term = vi.hoisted(() => ({
  written: [] as string[],
  opened: 0,
  disposed: 0,
  options: null as Record<string, unknown> | null,
  onData: null as ((data: string) => void) | null,
  dataDisposed: 0,
}));

vi.mock("@xterm/xterm", () => ({
  Terminal: class {
    constructor(options: Record<string, unknown>) {
      term.options = options;
    }
    open() {
      term.opened += 1;
    }
    write(chunk: string) {
      term.written.push(chunk);
    }
    focus() {}
    dispose() {
      term.disposed += 1;
    }
    onData(handler: (data: string) => void) {
      term.onData = handler;
      return {
        dispose: () => {
          term.dataDisposed += 1;
        },
      };
    }
  },
}));

vi.mock("@xterm/xterm/css/xterm.css", () => ({}));

const pty = vi.hoisted(() => ({
  written: [] as { id: string; data: string }[],
  resized: 0,
}));

vi.mock("../platform/tauri/pty", () => ({
  writePty: (id: string, data: string) => {
    pty.written.push({ id, data });
    return Promise.resolve();
  },
  resizePty: () => {
    pty.resized += 1;
    return Promise.resolve();
  },
  killPty: () => Promise.resolve(),
}));

const { RemoteControlTerminal } = await import("./RemoteControlTerminal");
const { createPtyFanout } = await import("./remoteControlSession");

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  term.written = [];
  term.opened = 0;
  term.disposed = 0;
  term.options = null;
  term.onData = null;
  term.dataDisposed = 0;
  pty.written = [];
  pty.resized = 0;
  container = document.createElement("div");
  document.body.appendChild(container);
  act(() => {
    root = createRoot(container);
  });
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

function render(
  fanout: ReturnType<typeof createPtyFanout>,
  replay: string,
  onClose = () => undefined,
) {
  act(() => {
    root.render(
      createElement(RemoteControlTerminal, {
        ptyId: "remote-control:s1",
        replay,
        attach: fanout.attach,
        cols: 120,
        rows: 40,
        onClose,
      }),
    );
  });
}

describe("the raw pty in front of the user", () => {
  it("opens on what is already on screen", () => {
    // Opening empty would hide the very prompt the user was sent here to answer.
    render(createPtyFanout(() => undefined), "Do you want to edit probe.txt?");

    expect(term.opened).toBe(1);
    expect(term.written).toEqual(["Do you want to edit probe.txt?"]);
  });

  it("is sized to the pty and never resizes it", () => {
    render(createPtyFanout(() => undefined), "");

    // The parser reads columns placed by absolute cursor moves, so a resize
    // would silently move every one of them.
    expect(term.options).toMatchObject({ cols: 120, rows: 40 });
    expect(pty.resized).toBe(0);
  });

  it("reads through the fan-out rather than subscribing", () => {
    const parsed: string[] = [];
    const fanout = createPtyFanout((chunk) => parsed.push(chunk));
    render(fanout, "");

    act(() => fanout.dispatch("live output"));

    expect(term.written).toContain("live output");
    // The parser is still fed; the view is an additional reader, not a
    // replacement for it.
    expect(parsed).toEqual(["live output"]);
    expect(fanout.viewerCount()).toBe(1);
  });

  it("gives up its reader on unmount, and only its own", () => {
    const parsed: string[] = [];
    const fanout = createPtyFanout((chunk) => parsed.push(chunk));
    render(fanout, "");

    act(() => root.unmount());

    expect(fanout.viewerCount()).toBe(0);
    act(() => fanout.dispatch("after unmount"));
    // The pty is still watched. A view that took the parser's place on its way
    // out would leave it watched by nobody, with no error anywhere.
    expect(parsed).toEqual(["after unmount"]);
    // Re-created in afterEach's unmount, which must stay harmless.
    act(() => {
      root = createRoot(container);
    });
  });

  it("types into the pty and not into the session", () => {
    render(createPtyFanout(() => undefined), "");

    act(() => term.onData?.("3"));

    expect(pty.written).toEqual([{ id: "remote-control:s1", data: "3" }]);
  });

  it("disposes the terminal and its input on unmount", () => {
    render(createPtyFanout(() => undefined), "");

    act(() => root.unmount());

    expect(term.disposed).toBe(1);
    expect(term.dataDisposed).toBe(1);
    act(() => {
      root = createRoot(container);
    });
  });
});
