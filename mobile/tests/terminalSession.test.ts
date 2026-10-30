import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { fromBase64, textBytes, toBase64 } from "@/features/terminal/base64";
import {
  TerminalSession,
  type TerminalFrame,
  type TerminalState,
} from "@/features/terminal/terminalSession";

const link = vi.hoisted(() => ({
  openWatch: vi.fn(),
  nextUpdates: vi.fn(),
  closeWatch: vi.fn(),
  request: vi.fn(),
}));
vi.mock("@/features/connection/nativeLink", () => link);

const pairing = {} as never;
const target = { projectId: "p", worktreePath: "/w" };
const size = { cols: 80, rows: 24 };
const hello = (over: Partial<TerminalFrame> = {}): TerminalFrame => ({
  shellId: "s1",
  seq: 0,
  reset: true,
  next: 1,
  ...over,
});

/** A desktop that answers each poll from a script, then waits. */
function desktop(script: (TerminalFrame[] | Error)[]) {
  let watches = 0;
  link.openWatch.mockImplementation(async () => ++watches);
  link.nextUpdates.mockImplementation(async () => {
    const next = script.shift();
    if (next instanceof Error) throw next;
    if (next) return next;
    return new Promise(() => {});
  });
}

function run(resume?: ConstructorParameters<typeof TerminalSession>[4]) {
  const output: [string, boolean][] = [];
  const states: TerminalState[] = [];
  const session = new TerminalSession(
    pairing,
    target,
    size,
    {
      onOutput: (data, reset) => output.push([data, reset]),
      onState: (state) => states.push(state),
    },
    resume,
  );
  void session.start();
  return { session, output, states };
}

beforeEach(() => {
  vi.useFakeTimers();
  link.openWatch.mockReset();
  link.nextUpdates.mockReset();
  link.closeWatch.mockReset().mockResolvedValue(undefined);
  link.request.mockReset().mockResolvedValue({ next: 2 });
});
afterEach(() => vi.useRealTimers());

describe("base64", () => {
  it("round-trips bytes of every length", () => {
    for (const text of ["", "a", "ab", "abc", "\x1b[A", "héllo ☃"]) {
      const bytes = textBytes(text);
      expect(toBase64(bytes)).toBe(Buffer.from(bytes).toString("base64"));
      expect(Array.from(fromBase64(toBase64(bytes)))).toEqual(
        Array.from(bytes),
      );
    }
  });
});

describe("a terminal session", () => {
  it("opens the shell in the project's folder and draws what arrives", async () => {
    desktop([
      [hello({ data: "aGk=", seq: 2 })],
      [{ shellId: "s1", seq: 5, data: "eW8=" }],
    ]);
    const { output, states } = run();
    await vi.advanceTimersByTimeAsync(0);
    expect(link.openWatch).toHaveBeenCalledWith(pairing, "terminal.watch", {
      projectId: "p",
      worktreePath: "/w",
      cols: 80,
      rows: 24,
    });
    expect(output).toEqual([
      ["aGk=", true],
      ["eW8=", false],
    ]);
    expect(states.at(-1)).toBe("live");
  });

  it("types in numbered batches, continuing from where the shell expects", async () => {
    desktop([[hello({ next: 7 })]]);
    const { session } = run();
    await vi.advanceTimersByTimeAsync(0);
    session.input(textBytes("l"));
    session.input(textBytes("s"));
    session.input(textBytes("\r"));
    await vi.advanceTimersByTimeAsync(20);
    const typed = link.request.mock.calls.filter(
      ([, method]) => method === "terminal.input",
    );
    expect(typed).toHaveLength(1);
    expect(typed[0][2]).toEqual({
      shellId: "s1",
      seq: 7,
      data: toBase64(textBytes("ls\r")),
    });
    session.input(textBytes("x"));
    await vi.advanceTimersByTimeAsync(20);
    expect(link.request.mock.calls.at(-1)![2].seq).toBe(8);
  });

  it("sends each key without waiting for the last one's answer", async () => {
    desktop([[hello({ next: 3 })]]);
    const { session } = run();
    await vi.advanceTimersByTimeAsync(0);
    link.request.mockImplementation(() => new Promise(() => {}));
    session.input(textBytes("a"));
    await vi.advanceTimersByTimeAsync(0);
    session.input(textBytes("b"));
    await vi.advanceTimersByTimeAsync(0);
    const typed = link.request.mock.calls.filter(
      ([, method]) => method === "terminal.input",
    );
    expect(typed.map(([, , params]) => params.seq)).toEqual([3, 4]);
  });

  it("sends typing that failed again under the same number", async () => {
    desktop([[hello()]]);
    const { session } = run();
    await vi.advanceTimersByTimeAsync(0);
    link.request.mockRejectedValueOnce(new Error("Desktop did not answer"));
    session.input(textBytes("a"));
    await vi.advanceTimersByTimeAsync(20);
    await vi.advanceTimersByTimeAsync(1100);
    const typed = link.request.mock.calls.filter(
      ([, method]) => method === "terminal.input",
    );
    expect(typed).toHaveLength(2);
    expect(typed[1][2]).toEqual(typed[0][2]);
  });

  it("tells the shell its size only when it changes", async () => {
    desktop([[hello()]]);
    const { session } = run();
    await vi.advanceTimersByTimeAsync(0);
    session.resize(80, 24);
    session.resize(100, 30);
    await vi.advanceTimersByTimeAsync(0);
    const resized = link.request.mock.calls.filter(
      ([, method]) => method === "terminal.resize",
    );
    expect(resized.map(([, , params]) => params)).toEqual([
      { shellId: "s1", cols: 100, rows: 30 },
    ]);
  });

  it("reattaches after a dropped connection from the last output it drew", async () => {
    desktop([
      [hello({ data: "YQ==", seq: 1 })],
      new Error("Desktop stopped answering"),
    ]);
    const { states } = run();
    await vi.advanceTimersByTimeAsync(0);
    await vi.advanceTimersByTimeAsync(600);
    expect(states).toContain("reconnecting");
    expect(link.openWatch).toHaveBeenCalledTimes(2);
    expect(link.openWatch.mock.calls[1][2]).toMatchObject({
      shellId: "s1",
      after: 1,
    });
  });

  it("carries on from a saved shell, and gives up when it has ended", async () => {
    link.openWatch.mockRejectedValue(new Error("That terminal has ended"));
    const { states } = run({ shellId: "old", seq: 40, next: 9 });
    await vi.advanceTimersByTimeAsync(0);
    expect(link.openWatch.mock.calls[0][2]).toMatchObject({
      shellId: "old",
      after: 40,
    });
    expect(states.at(-1)).toBe("ended");
    expect(link.openWatch).toHaveBeenCalledTimes(1);
  });

  it("draws the shell again from the start when the screen was lost", async () => {
    let watches = 0;
    const polls: ((frames: TerminalFrame[]) => void)[] = [];
    const rejects: ((error: Error) => void)[] = [];
    link.openWatch.mockImplementation(async () => ++watches);
    link.nextUpdates.mockImplementation(
      () =>
        new Promise((resolve, reject) => {
          polls.push(resolve);
          rejects.push(reject);
        }),
    );
    // Closing a watch ends the poll waiting on it.
    link.closeWatch.mockImplementation(async () =>
      rejects.pop()?.(new Error("Watch closed")),
    );
    const { session } = run();
    await vi.advanceTimersByTimeAsync(0);
    polls.shift()!([hello({ data: "YQ==", seq: 4 })]);
    await vi.advanceTimersByTimeAsync(0);
    session.replay();
    await vi.advanceTimersByTimeAsync(600);
    expect(link.openWatch).toHaveBeenCalledTimes(2);
    expect(link.openWatch.mock.calls[1][2]).toMatchObject({
      shellId: "s1",
      after: 0,
    });
  });

  it("reports the shell exiting and closes the shell when asked", async () => {
    desktop([[hello(), { shellId: "s1", seq: 3, exit: 0 }]]);
    const { session, states } = run();
    await vi.advanceTimersByTimeAsync(0);
    expect(states.at(-1)).toBe("ended");
    await session.close();
    expect(link.request).toHaveBeenCalledWith(pairing, "terminal.close", {
      shellId: "s1",
    });
  });
});
