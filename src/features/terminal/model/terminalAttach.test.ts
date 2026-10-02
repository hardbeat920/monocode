import { Terminal } from "@xterm/headless";
import { afterEach, describe, expect, it } from "vitest";
import { createAttachGate } from "./terminalAttach";

// What the backend hands a reloaded view, captured from bash and Vim on a
// real PTY (see `pty_reattach_tests.rs`).
const SHELL_PROMPT = "\x1b[?2004h";
const VIM = "\x1b[?1049h\x1b[?1h\x1b=\x1b[?2004h\x1b[?1004h";

const terms: Terminal[] = [];

function view() {
  const term = new Terminal({ cols: 80, rows: 24, allowProposedApi: true });
  terms.push(term);
  const sent: string[] = [];
  term.onData((data) => sent.push(data));
  const gate = createAttachGate(term, (chunk) => term.write(chunk));
  return { term, sent, gate };
}

/** `Terminal.paste` from xterm's browser build, which headless lacks. */
function paste(term: Terminal, text: string) {
  const prepared = text.replace(/\r?\n/g, "\r");
  term.input(
    term.modes.bracketedPasteMode
      ? `\x1b[200~${prepared}\x1b[201~`
      : prepared,
    true,
  );
}

function parsed(term: Terminal): Promise<void> {
  return new Promise((resolve) => term.write("", resolve));
}

const bytes = (text: string) => new TextEncoder().encode(text);

afterEach(() => {
  for (const term of terms.splice(0)) term.dispose();
});

describe("reattaching a terminal view", () => {
  it("wraps a paste once the shell's bracketed paste is restored", async () => {
    const { term, sent, gate } = view();
    gate.open(SHELL_PROMPT);
    await parsed(term);
    paste(term, "echo one\necho two\n");
    expect(sent).toEqual(["\x1b[200~echo one\recho two\r\x1b[201~"]);
  });

  it("sends a paste bare to a view left at default modes", async () => {
    // The bug: a fresh xterm types both lines into the shell, which runs
    // them as they land.
    const { term, sent, gate } = view();
    gate.open("");
    await parsed(term);
    paste(term, "echo one\necho two\n");
    expect(sent).toEqual(["echo one\recho two\r"]);
  });

  it("puts a TUI's screen and key modes back", async () => {
    const { term, sent, gate } = view();
    gate.open(VIM);
    await parsed(term);
    expect(term.buffer.active.type).toBe("alternate");
    expect(term.modes.applicationCursorKeysMode).toBe(true);
    expect(term.modes.applicationKeypadMode).toBe(true);
    expect(term.modes.bracketedPasteMode).toBe(true);
    expect(term.modes.sendFocusMode).toBe(true);
    term.input("\x1b[A", true);
    expect(sent).toEqual(["\x1b[A"]);
  });

  it("restores mouse reporting and cursor visibility", async () => {
    const { term, gate } = view();
    gate.open("\x1b[?1049h\x1b[?1002h\x1b[?1006h\x1b[?25l");
    await parsed(term);
    expect(term.modes.mouseTrackingMode).toBe("drag");
    expect(term.buffer.active.type).toBe("alternate");
  });

  it("holds input until the modes are in, then lets it through", async () => {
    const { term, sent, gate } = view();
    paste(term, "too early\n");
    expect(sent).toEqual([]);
    gate.open(SHELL_PROMPT);
    await parsed(term);
    paste(term, "in time\n");
    expect(sent).toEqual(["\x1b[200~in time\r\x1b[201~"]);
  });

  it("paints held output over the restored modes, in order", async () => {
    const { term, gate } = view();
    // Vim's SIGWINCH redraw can reach the page before `pty_spawn` answers.
    gate.data(bytes("\x1b[H\x1b[2J~"));
    gate.data(bytes("\x1b[2;1H~"));
    expect(term.buffer.active.getLine(0)?.translateToString(true)).toBe("");
    gate.open(VIM);
    await parsed(term);
    // On the alternate screen, where Vim draws: nothing landed on the
    // shell's screen underneath.
    expect(term.buffer.active.type).toBe("alternate");
    expect(term.buffer.active.getLine(0)?.translateToString(true)).toBe("~");
    expect(term.buffer.active.getLine(1)?.translateToString(true)).toBe("~");
    expect(term.buffer.normal.getLine(0)?.translateToString(true)).toBe("");
    gate.data(bytes("\x1b[3;1H~"));
    await parsed(term);
    expect(term.buffer.active.getLine(2)?.translateToString(true)).toBe("~");
  });

  it("answers terminal queries in held output", async () => {
    // A fresh shell may ask for device attributes before `pty_spawn`
    // returns; the reply must not be swallowed while input is off.
    const { term, sent, gate } = view();
    gate.data(bytes("\x1b[c"));
    gate.open("");
    await parsed(term);
    expect(sent.join("")).toMatch(/^\x1b\[\?[\d;]+c$/);
  });

  it("stays shut once the view is gone", async () => {
    const { term, sent, gate } = view();
    gate.data(bytes("hello"));
    gate.close();
    gate.open(SHELL_PROMPT);
    await parsed(term);
    paste(term, "x");
    expect(sent).toEqual([]);
    expect(term.buffer.active.getLine(0)?.translateToString(true)).toBe("");
  });
});
