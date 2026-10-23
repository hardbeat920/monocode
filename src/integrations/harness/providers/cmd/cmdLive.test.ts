import { beforeEach, describe, expect, it, vi } from "vitest";
import type { HarnessEvent } from "../../core/types";

type Watch = {
  onLine: (line: string) => void;
  onExit: (code: number | null) => void;
};

const state = vi.hoisted(() => ({
  spawned: [] as string[][],
  watches: [] as Watch[],
  textContent: "",
}));

vi.mock("../../core/child", () => ({
  resolveCmdBinary: async () => ({ path: "/fake/cmd" }),
  readHarnessTextFile: async () => state.textContent,
  spawnChild: async (
    _sessionId: string,
    _path: string,
    args: string[],
    _cwd: string,
    _account?: unknown,
    _provider?: string,
  ) => {
    state.spawned.push(args);
  },
  killChild: async () => undefined,
  unwatchChild: () => undefined,
  watchChild: (
    _sessionId: string,
    onLine: (line: string) => void,
    onExit: (code: number | null) => void,
  ) => {
    state.watches.push({ onLine, onExit });
  },
}));

const { sendCmdTurn, stopCmdSession } = await import("./cmd");

function send(sessionId: string, text: string) {
  const events: HarnessEvent[] = [];
  const turn = sendCmdTurn({
    sessionId,
    cwd: "/repo",
    text,
    model: "cmd:deepseek/deepseek-v4-flash",
    runtimeMode: "auto",
    onEvent: (event) => events.push(event),
  });
  return { events, turn };
}

function emit(frame: Record<string, unknown>): void {
  const watch = state.watches.at(-1);
  if (!watch) throw new Error("no active cmd child watch");
  watch.onLine(JSON.stringify(frame));
}

function exit(code: number | null): void {
  const watch = state.watches.at(-1);
  if (!watch) throw new Error("no active cmd child watch");
  watch.onExit(code);
}

const waitFor = async (pred: () => boolean, label: string) => {
  for (let i = 0; i < 400; i += 1) {
    if (pred()) return;
    await new Promise((resolve) => setTimeout(resolve, 5));
  }
  throw new Error(`timed out waiting for ${label}`);
};

describe("cmd live turn recovery", () => {
  beforeEach(() => {
    state.spawned.length = 0;
    state.watches.length = 0;
    state.textContent = "";
  });

  it(
    "retries a dropped connection and resumes with a continuation nudge",
    async () => {
      const { events, turn } = send("cmd-retry-bound", "fix the bug");
      await waitFor(() => state.spawned.length === 1, "first spawn");
      expect(state.spawned[0]).toContain("fix the bug");
      expect(state.spawned[0]).toContain("--yolo");
      expect(state.spawned[0]).not.toContain("--resume");

      emit({
        type: "event",
        event: { type: "run_start", sessionId: "sess-1" },
      });
      // Tools already ran: the old retry gate refused to recover past this
      // point even though the provider session can simply resume.
      emit({
        type: "event",
        event: {
          type: "tool_queued",
          toolCallId: "call_1",
          toolName: "shell_command",
          input: { command: "npm test" },
        },
      });
      emit({
        type: "event",
        event: {
          type: "tool_completed",
          toolCallId: "call_1",
          toolName: "shell_command",
          input: { command: "npm test" },
        },
      });
      emit({
        type: "result",
        subtype: "error",
        sessionId: "sess-1",
        error:
          "Error: Unable to connect to the API. Please check your network connection.",
      });

      await waitFor(
        () =>
          events.some(
            (event) =>
              event.type === "status" && event.text.includes("Restarting the turn"),
          ),
        "retry status",
      );
      await waitFor(() => state.spawned.length === 2, "second spawn");

      const retry = state.spawned[1] ?? [];
      expect(retry).toContain("--resume");
      expect(retry).toContain("sess-1");
      const retryPrompt = retry.join("\n");
      expect(retryPrompt).toContain("Continue exactly where you left off");
      expect(retryPrompt).not.toContain("fix the bug");

      emit({ type: "result", subtype: "success", sessionId: "sess-1" });
      await turn;
      expect(events.some((event) => event.type === "message.completed")).toBe(true);
      expect(events.some((event) => event.type === "session.error")).toBe(false);
    },
    10_000,
  );

  it(
    "replays the original prompt when no provider session was bound",
    async () => {
      const { events, turn } = send("cmd-retry-replay", "add tests");
      await waitFor(() => state.spawned.length === 1, "first spawn");

      // Exit 6 = connection failure, before any session id reached the client.
      exit(6);

      await waitFor(() => state.spawned.length === 2, "second spawn");
      expect(state.spawned[1]).toContain("add tests");
      expect(state.spawned[1]).not.toContain("--resume");

      emit({ type: "result", subtype: "success" });
      await turn;
      expect(events.some((event) => event.type === "session.error")).toBe(false);
    },
    10_000,
  );

  it("does not retry non-transient failures", async () => {
    const { events, turn } = send("cmd-retry-auth", "hi");
    await waitFor(() => state.spawned.length === 1, "first spawn");

    emit({
      type: "result",
      subtype: "error",
      error: "CommandCode is not authenticated. Run `cmd login` in a terminal, then retry.",
    });

    await expect(turn).rejects.toThrow(/not authenticated/);
    expect(state.spawned).toHaveLength(1);
    expect(
      events.some(
        (event) =>
          event.type === "session.error" && /not authenticated/.test(event.message),
      ),
    ).toBe(true);
  });

  it("streams tool output from tool_update and keeps the completed result", async () => {
    const { events, turn } = send("cmd-tool-output", "run the tests");
    await waitFor(() => state.spawned.length === 1, "first spawn");

    emit({
      type: "event",
      event: {
        type: "tool_queued",
        toolCallId: "call_9",
        toolName: "shell_command",
        input: { command: "npm test" },
      },
    });
    emit({
      type: "event",
      event: {
        type: "tool_update",
        toolCallId: "call_9",
        toolName: "shell_command",
        partial: [{ type: "text", text: "running…" }],
      },
    });
    emit({
      type: "event",
      event: {
        type: "tool_completed",
        toolCallId: "call_9",
        toolName: "shell_command",
        result: [{ type: "text", text: "all tests passed" }],
        deferred: false,
      },
    });

    expect(events).toContainEqual(
      expect.objectContaining({
        type: "tool.updated",
        callId: "call_9",
        status: "in_progress",
        detail: "running…",
      }),
    );
    expect(events).toContainEqual(
      expect.objectContaining({
        type: "tool.updated",
        callId: "call_9",
        status: "completed",
        detail: "all tests passed",
      }),
    );

    emit({ type: "result", subtype: "success" });
    await turn;
  });

  it("settles a pending turn when the session is stopped", async () => {
    const { turn } = send("cmd-stop", "hi");
    await waitFor(() => state.spawned.length === 1, "first spawn");

    await stopCmdSession("cmd-stop");
    await expect(turn).resolves.toBeUndefined();
  });

  it("keeps inline attachment content under the argv budget", async () => {
    state.textContent = "x".repeat(20_000);
    const events: HarnessEvent[] = [];
    const turn = sendCmdTurn({
      sessionId: "cmd-inline-budget",
      cwd: "/repo",
      text: "summarize",
      model: "cmd:deepseek/deepseek-v4.1-flash",
      runtimeMode: "auto",
      attachments: [
        {
          id: "a",
          name: "a.md",
          mimeType: "text/markdown",
          kind: "file",
          size: 20_000,
          path: "/repo/a.md",
        },
        {
          id: "b",
          name: "b.md",
          mimeType: "text/markdown",
          kind: "file",
          size: 20_000,
          path: "/repo/b.md",
        },
      ],
      onEvent: (event) => events.push(event),
    });
    await waitFor(() => state.spawned.length === 1, "first spawn");

    const prompt = state.spawned[0]?.find((arg) => arg.includes("summarize")) ?? "";
    expect(prompt.length).toBeLessThan(30_000);
    expect(prompt).toContain("/repo/b.md");

    emit({ type: "result", subtype: "success" });
    await turn;
  });

  it("forwards the CLI's own recovery notices as keyed statuses", async () => {
    const { events, turn } = send("cmd-retry-notices", "hi");
    await waitFor(() => state.spawned.length === 1, "first spawn");

    emit({ type: "event", event: { type: "api_retry", attempt: 3 } });
    emit({
      type: "event",
      event: { type: "stream_restart", attempt: 1, maxAttempts: 3 },
    });

    expect(events).toContainEqual({
      type: "status",
      key: "cmd-connection",
      text: "Connection issue. Retrying… (attempt 3)",
    });
    expect(events).toContainEqual({
      type: "status",
      key: "cmd-connection",
      text: "Connection dropped mid-response. Retrying… (1/3)",
    });

    emit({ type: "result", subtype: "success" });
    await turn;
  });
});
