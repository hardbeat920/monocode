import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { HarnessEvent, SendTurnInput } from "../../core/types";

const sent: Record<string, unknown>[] = [];
const spawned: string[][] = [];
let onLine: ((line: string) => void) | undefined;
let onExit: ((code: number | null) => void) | undefined;
const spawnChild = vi.fn(async (_id: string, _path: string, args: string[]) => {
  spawned.push(args);
});
const writeChild = vi.fn(async (_id: string, line: string) => {
  sent.push(JSON.parse(line));
});
vi.mock("../../core/child", () => ({
  resolveClaudeBinary: async () => ({ path: "/fake/claude" }),
  spawnChild,
  killChild: async () => undefined,
  unwatchChild: () => undefined,
  watchChild: (_id: string, line: typeof onLine, exit: typeof onExit) => {
    onLine = line;
    onExit = exit;
  },
  writeChild,
}));
const {
  sendClaudeTurn,
  steerClaudeTurn,
  respondClaudeQuestion,
  cancelClaudeTurn,
  stopClaudeSession,
  setClaudeBinaryResolver,
  __claudeTestReset,
} = await import("./claude");

const tick = async () => {
  for (let i = 0; i < 12; i++) await Promise.resolve();
};
const emit = (record: Record<string, unknown>) =>
  onLine!(JSON.stringify(record));
const input = (events: HarnessEvent[], text = "first"): SendTurnInput => ({
  sessionId: "audit-session",
  cwd: "/repo",
  model: "claude:claude-haiku-4-5",
  runtimeMode: "supervised",
  text,
  attachments: [],
  onEvent: (event) => events.push(event),
});
async function started(events: HarnessEvent[]) {
  const promise = sendClaudeTurn(input(events));
  await tick();
  emit({
    type: "control_response",
    response: { subtype: "success", request_id: "monocode_2", response: {} },
  });
  await tick();
  expect(sent.filter((m) => m.type === "user")).toHaveLength(1);
  return { promise };
}

beforeEach(() => {
  __claudeTestReset();
  sent.length = 0;
  spawned.length = 0;
  onLine = undefined;
  onExit = undefined;
  spawnChild.mockReset();
  spawnChild.mockImplementation(async (_id, _path, args) => {
    spawned.push(args);
  });
  writeChild.mockReset();
  writeChild.mockImplementation(async (_id, line) => {
    sent.push(JSON.parse(line));
  });
});
afterEach(async () => {
  await stopClaudeSession("audit-session");
  vi.clearAllTimers();
  vi.useRealTimers();
  __claudeTestReset();
});

describe("official v0.7.0 lifecycle audit", () => {
  it("keeps an MCP form open after invalid input and sends the corrected typed answer", async () => {
    const events: HarnessEvent[] = [];
    const first = await started(events);
    emit({
      type: "control_request",
      request_id: "form",
      request: {
        subtype: "elicitation",
        mode: "form",
        requested_schema: {
          type: "object",
          properties: { count: { type: "integer", minimum: 1 } },
          required: ["count"],
        },
      },
    });
    await tick();
    let question = [...events]
      .reverse()
      .find((event) => event.type === "question.asked")!;
    if (question.type !== "question.asked") throw new Error("No form");
    respondClaudeQuestion("audit-session", question.requestId, {
      kind: "answered",
      answers: { __mcp_action: ["accept"] },
      custom: { count: "0" },
    });
    await tick();
    expect(
      sent.some(
        (record) =>
          (record.response as Record<string, unknown>)?.request_id === "form",
      ),
    ).toBe(false);
    question = [...events]
      .reverse()
      .find((event) => event.type === "question.asked")!;
    if (question.type !== "question.asked")
      throw new Error("No corrected form");
    respondClaudeQuestion("audit-session", question.requestId, {
      kind: "answered",
      answers: { __mcp_action: ["accept"] },
      custom: { count: "2" },
    });
    await tick();
    expect(sent).toContainEqual({
      type: "control_response",
      response: {
        subtype: "success",
        request_id: "form",
        response: { action: "accept", content: { count: 2 } },
      },
    });
    emit({ type: "result", subtype: "success", is_error: false });
    await first.promise;
  });
  it("cancels binary discovery without cancelling the next send", async () => {
    let resolve!: (value: { path: string }) => void;
    setClaudeBinaryResolver(
      () =>
        new Promise((done) => {
          resolve = done;
        }),
    );
    const events: HarnessEvent[] = [];
    const first = sendClaudeTurn(input(events));
    await tick();
    await cancelClaudeTurn("audit-session");
    resolve({ path: "/fake/claude" });
    await first;
    expect(spawned).toHaveLength(0);
    setClaudeBinaryResolver(async () => ({ path: "/fake/claude" }));
    const second = await started(events);
    emit({ type: "result", subtype: "success", is_error: false });
    await second.promise;
  });

  it("does not send a prompt that was cancelled during initialization", async () => {
    vi.useFakeTimers();
    const events: HarnessEvent[] = [];
    const promise = sendClaudeTurn(input(events));
    await tick();
    expect(sent.some((m) => m.type === "control_request")).toBe(true);
    await cancelClaudeTurn("audit-session");
    emit({
      type: "control_response",
      response: { subtype: "success", request_id: "monocode_2", response: {} },
    });
    await tick();
    await vi.advanceTimersByTimeAsync(8_001);
    await promise;
    expect(sent.filter((m) => m.type === "user")).toHaveLength(0);
  });

  it("does not complete a new turn with the cancelled turn's delayed result", async () => {
    const events: HarnessEvent[] = [];
    const first = await started(events);
    const oldLine = onLine!;
    await cancelClaudeTurn("audit-session");
    await first.promise;
    let settled = false;
    const second = sendClaudeTurn(input(events, "second"));
    void second.then(() => {
      settled = true;
    });
    await tick();
    emit({
      type: "control_response",
      response: { subtype: "success", request_id: "monocode_2", response: {} },
    });
    await tick();
    expect(sent.filter((m) => m.type === "user")).toHaveLength(2);
    oldLine(
      JSON.stringify({
        type: "result",
        subtype: "error_during_execution",
        is_error: true,
        errors: ["Interrupted"],
        terminal_reason: "aborted_streaming",
      }),
    );
    await tick();
    expect(settled).toBe(false);
  });

  it("reports API errors carried by a success result", async () => {
    const events: HarnessEvent[] = [];
    const first = await started(events);
    emit({
      type: "result",
      subtype: "success",
      is_error: true,
      result: "API Error: 529 Overloaded",
      errors: [],
    });
    await first.promise;
    expect(events).toContainEqual({
      type: "session.error",
      message: "API Error: 529 Overloaded",
    });
  });

  it("keeps the MonoCode turn active for a queued steer message", async () => {
    const events: HarnessEvent[] = [];
    const first = await started(events);
    let settled = false;
    void first.promise.then(() => {
      settled = true;
    });
    await steerClaudeTurn(input(events, "second"));
    emit({
      type: "result",
      subtype: "success",
      result: "FIRST",
      is_error: false,
    });
    await tick();
    expect(settled).toBe(false);
  });

  it("waits for the follow-up of a background task that finished before the first result", async () => {
    const events: HarnessEvent[] = [];
    const first = await started(events);
    let settled = false;
    void first.promise.then(() => {
      settled = true;
    });
    emit({
      type: "system",
      subtype: "task_started",
      task_id: "short-bash",
      task_type: "local_bash",
      is_backgrounded: true,
      description: "Audit verification",
    });
    emit({ type: "system", subtype: "background_tasks_changed", tasks: [] });
    emit({
      type: "system",
      subtype: "task_updated",
      task_id: "short-bash",
      patch: { status: "completed" },
    });
    emit({
      type: "system",
      subtype: "task_notification",
      task_id: "short-bash",
      status: "completed",
      summary: "AUDIT_DONE",
    });
    emit({ type: "result", subtype: "success", is_error: false });
    await tick();
    expect(settled).toBe(false);
  });

  it("rejects an initialization error rather than sending the prompt", async () => {
    const events: HarnessEvent[] = [];
    const promise = sendClaudeTurn(input(events));
    void promise.catch(() => undefined);
    await tick();
    emit({
      type: "control_response",
      response: {
        subtype: "error",
        request_id: "monocode_2",
        error: "Initialization failed",
      },
    });
    await tick();
    expect(sent.filter((m) => m.type === "user")).toHaveLength(0);
  });

  it("does not report initialized when no initialize acknowledgement arrives", async () => {
    vi.useFakeTimers();
    const events: HarnessEvent[] = [];
    const promise = sendClaudeTurn(input(events));
    void promise.catch(() => undefined);
    await tick();
    await vi.advanceTimersByTimeAsync(8_001);
    expect(events.some((event) => event.type === "session.started")).toBe(
      false,
    );
  });

  it("does not mark a child that exited just after initialization as started", async () => {
    const events: HarnessEvent[] = [];
    const promise = sendClaudeTurn(input(events));
    void promise.catch(() => undefined);
    await tick();
    emit({
      type: "control_response",
      response: { subtype: "success", request_id: "monocode_2", response: {} },
    });
    writeChild.mockRejectedValue(new Error("Harness process is not running"));
    onExit!(1);
    await expect(promise).rejects.toThrow("stopped during initialization");
    expect(events).toContainEqual({ type: "session.ended", code: 1 });
    expect(events.some((event) => event.type === "session.started")).toBe(
      false,
    );
  });

  it("does not silently succeed when an interrupt cannot reach Claude", async () => {
    const events: HarnessEvent[] = [];
    const first = await started(events);
    writeChild.mockRejectedValue(new Error("Write failed"));
    await expect(cancelClaudeTurn("audit-session")).rejects.toThrow(
      "Write failed",
    );
    await first.promise;
  });

  it("shows MCP form elicitation instead of silently cancelling it", async () => {
    const events: HarnessEvent[] = [];
    await started(events);
    emit({
      type: "control_request",
      request_id: "mcp-form",
      request: {
        subtype: "elicitation",
        mcp_server_name: "audit",
        message: "Provide the synthetic audit label.",
        mode: "form",
        requested_schema: {
          type: "object",
          properties: { label: { type: "string" } },
          required: ["label"],
        },
      },
    });
    await tick();
    expect(events.some((event) => event.type === "question.asked")).toBe(true);
    expect(
      sent.some(
        (record) =>
          record.type === "control_response" &&
          (record.response as Record<string, unknown>)?.request_id ===
            "mcp-form",
      ),
    ).toBe(false);
  });
});
