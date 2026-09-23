import { beforeEach, describe, expect, it, vi } from "vitest";

let onLine: ((line: string) => void) | undefined;
let onExit: ((code: number | null) => void) | undefined;
let onStderr: ((line: string) => void) | undefined;
let spawned: { command: string; args: string[]; cwd: string } | undefined;
let killed = 0;
let autoExit = true;

vi.mock("../../core/child", () => ({
  resolveMuseBinary: async () => ({ path: "/fake/muse" }),
  spawnChild: async (sessionId: string, command: string, args: string[], cwd: string) => {
    spawned = { command, args, cwd };
  },
  killChild: async () => {
    killed += 1;
    // Mirror the real bridge: killing the child delivers its exit event.
    if (!autoExit) return;
    const exit = onExit;
    onExit = undefined;
    exit?.(null);
  },
  unwatchChild: () => undefined,
  watchChild: (
    _id: string,
    line: (l: string) => void,
    exit: (c: number | null) => void,
    stderr?: (l: string) => void,
  ) => {
    onLine = line;
    onExit = exit;
    onStderr = stderr;
  },
}));

const {
  sendMuseTurn,
  cancelMuseTurn,
  bindMuseSession,
  forgetMuseSession,
  stopMuseSession,
} = await import("./muse");
import type { HarnessEvent } from "../../core/types";

const SID = "01a0cfef-7b4a-7a21-8da4-98066fcadbd7";

function line(payloadType: string, payload: unknown): string {
  return JSON.stringify({
    stream: { kind: "session", id: SID },
    payload_type: payloadType,
    payload,
  });
}

function baseInput(events: HarnessEvent[]) {
  return {
    sessionId: "thread-1",
    cwd: "/tmp/work",
    model: "muse:muse-spark-1.3-contributor",
    modelSettings: {},
    runtimeMode: "supervised" as const,
    text: "hi",
    onEvent: (event: HarnessEvent) => events.push(event),
  };
}

beforeEach(() => {
  onLine = undefined;
  onExit = undefined;
  onStderr = undefined;
  spawned = undefined;
  killed = 0;
  autoExit = true;
});

describe("muse turns", () => {
  it("streams a turn, binds the provider session and resumes it", async () => {
    const events: HarnessEvent[] = [];
    const turn = sendMuseTurn(baseInput(events));
    await new Promise((r) => setTimeout(r, 5));
    expect(spawned?.command).toBe("/fake/muse");
    expect(spawned?.args.slice(0, 4)).toEqual(["exec", "--json", "--workspace", "/tmp/work"]);
    expect(spawned?.args).not.toContain("--session-id");

    onLine!(line("run.output.delta", { kind: "run_output_delta", text: "hello" }));
    onLine!(line("run.terminal.completed", { kind: "run_terminal", terminal: "completed" }));
    onExit!(0);
    await turn;

    expect(events).toContainEqual({ type: "message.delta", text: "hello" });
    expect(events).toContainEqual({ type: "message.completed" });
    expect(events).toContainEqual({ type: "session.providerBound", providerSessionId: SID });

    const events2: HarnessEvent[] = [];
    const turn2 = sendMuseTurn(baseInput(events2));
    await new Promise((r) => setTimeout(r, 5));
    const sessionFlag = spawned?.args.indexOf("--session-id") ?? -1;
    expect(sessionFlag).toBeGreaterThan(-1);
    expect(spawned?.args[sessionFlag + 1]).toBe(SID);
    onLine!(line("run.terminal.completed", { kind: "run_terminal", terminal: "completed" }));
    onExit!(0);
    await turn2;
  });

  it("fails the turn when the child exits early", async () => {
    const events: HarnessEvent[] = [];
    await expect(
      (async () => {
        const turn = sendMuseTurn(baseInput(events));
        await new Promise((r) => setTimeout(r, 5));
        onExit!(1);
        await turn;
      })(),
    ).rejects.toThrow(/exited before finishing/);
  });

  it("cancels without failing", async () => {
    const events: HarnessEvent[] = [];
    const turn = sendMuseTurn(baseInput(events));
    await new Promise((r) => setTimeout(r, 5));
    await cancelMuseTurn("thread-1");
    expect(killed).toBeGreaterThan(0);
    await turn;
    expect(events).not.toContainEqual({ type: "message.completed" });
  });

  it("clears a stale cancel flag on stop so the next turn runs", async () => {
    // Cancel-then-stop while idle leaves no child to consume the flag.
    await cancelMuseTurn("thread-1");
    await stopMuseSession("thread-1");
    const events: HarnessEvent[] = [];
    const turn = sendMuseTurn(baseInput(events));
    await new Promise((r) => setTimeout(r, 5));
    expect(spawned?.command).toBe("/fake/muse");
    onLine!(line("run.terminal.completed", { kind: "run_terminal", terminal: "completed" }));
    onExit!(0);
    await turn;
    expect(events).toContainEqual({ type: "message.completed" });
  });

  it("reports auth failures with the login hint", async () => {
    const events: HarnessEvent[] = [];
    await expect(
      (async () => {
        const turn = sendMuseTurn(baseInput(events));
        await new Promise((r) => setTimeout(r, 5));
        onStderr!("not logged in, run `muse login`");
        onExit!(1);
        await turn;
      })(),
    ).rejects.toThrow(/muse login/);
  });

  it("starts fresh when the restored session belongs to another cwd", async () => {
    bindMuseSession("thread-9", SID, "/tmp/other");
    const events: HarnessEvent[] = [];
    const turn = sendMuseTurn({ ...baseInput(events), sessionId: "thread-9" });
    await new Promise((r) => setTimeout(r, 5));
    expect(spawned?.args).not.toContain("--session-id");
    onLine!(line("run.terminal.completed", { kind: "run_terminal", terminal: "completed" }));
    onExit!(0);
    await turn;
  });

  it("surfaces failed terminals as errors", async () => {
    const events: HarnessEvent[] = [];
    await expect(
      (async () => {
        const turn = sendMuseTurn(baseInput(events));
        await new Promise((r) => setTimeout(r, 5));
        onLine!(
          line("run.terminal.failed", {
            kind: "run_terminal",
            terminal: "failed",
            reason: "boom",
          }),
        );
        await turn;
      })(),
    ).rejects.toThrow(/ended \(failed\): boom/);
    expect(events).toContainEqual({ type: "message.completed" });
    expect(events).toContainEqual({
      type: "session.error",
      message: "Muse turn ended (failed): boom",
    });
  });

  it("mutes late output after cancel", async () => {
    // Hold the exit back so the late line lands while the flag is still set,
    // mirroring output that arrives between kill and process death.
    autoExit = false;
    const events: HarnessEvent[] = [];
    const turn = sendMuseTurn(baseInput(events));
    await new Promise((r) => setTimeout(r, 5));
    onLine!(line("run.output.delta", { kind: "run_output_delta", text: "partial" }));
    await cancelMuseTurn("thread-1");
    onLine!(line("run.terminal.completed", { kind: "run_terminal", terminal: "completed" }));
    onExit!(null);
    await turn;
    expect(events).toContainEqual({ type: "message.delta", text: "partial" });
    expect(events).not.toContainEqual({ type: "message.completed" });
  });

  it("binds restored sessions and forgets them", async () => {
    bindMuseSession("thread-9", SID, "/tmp/work");
    const events: HarnessEvent[] = [];
    const turn = sendMuseTurn({ ...baseInput(events), sessionId: "thread-9" });
    await new Promise((r) => setTimeout(r, 5));
    expect(spawned?.args).toContain(SID);
    onLine!(line("run.terminal.completed", { kind: "run_terminal", terminal: "completed" }));
    onExit!(0);
    await turn;
    await forgetMuseSession("thread-9");
  });
});
