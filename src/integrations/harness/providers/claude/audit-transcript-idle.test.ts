import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { HarnessEvent } from "../../core/types";

const sent: Record<string, unknown>[] = [];
let onLine: ((line: string) => void) | undefined;
const killChild = vi.fn(async () => undefined);
let toolSequence = 0;
vi.mock("../../core/child", () => ({
  resolveClaudeBinary: async () => ({ path: "/fake/claude" }),
  spawnChild: async () => undefined,
  killChild,
  unwatchChild: () => undefined,
  watchChild: (_id: string, line: (line: string) => void) => {
    onLine = line;
  },
  writeChild: async (_id: string, line: string) => {
    sent.push(JSON.parse(line));
  },
}));
vi.mock("@tauri-apps/api/core", () => ({
  isTauri: () => false,
  invoke: vi.fn(),
}));
const { __claudeTestReset, stopClaudeSession, claudeSessionNeedsProcess } =
  await import("./claude");
const { claudeAdapter } = await import("./claudeAdapter");
const { registerHarness, sendHarnessTurn, resetHarnessIdlePark } =
  await import("../../core/registry");
const tick = async () => {
  for (let i = 0; i < 30; i++) await Promise.resolve();
};
const emit = (record: Record<string, unknown>) =>
  onLine!(JSON.stringify(record));

async function startIdleTurn() {
  const turn = sendHarnessTurn({
    harness: "claude",
    sessionId: "audit-idle",
    cwd: "/repo",
    model: "claude:claude-haiku-4-5",
    runtimeMode: "supervised",
    text: "Schedule reminders",
    attachments: [],
    onEvent: () => undefined,
  });
  await tick();
  emit({
    type: "control_response",
    response: { subtype: "success", request_id: "monocode_2", response: {} },
  });
  emit({ type: "system", subtype: "init", session_id: "provider-idle" });
  await tick();
  return { turn };
}

function completeTool(
  name: string,
  input: Record<string, unknown>,
  result: Record<string, unknown> | undefined,
  text = "Synthetic tool result",
) {
  const id = `call:${name}:${toolSequence++}`;
  emit({
    type: "assistant",
    message: { content: [{ type: "tool_use", id, name, input }] },
  });
  emit({
    type: "user",
    tool_use_result: result,
    message: {
      content: [
        {
          type: "tool_result",
          tool_use_id: id,
          content: text,
        },
      ],
    },
  });
}

beforeEach(() => {
  vi.useFakeTimers();
  sent.length = 0;
  toolSequence = 0;
  killChild.mockClear();
  __claudeTestReset();
  resetHarnessIdlePark();
  registerHarness(claudeAdapter);
});
afterEach(async () => {
  resetHarnessIdlePark();
  await stopClaudeSession("audit-idle");
  __claudeTestReset();
  vi.clearAllTimers();
  vi.useRealTimers();
});

describe("review probe for native activity after an ordinary result", () => {
  it.each([false, true])(
    "reads a native cron ID from text with recurring set to %s",
    async (recurring) => {
      const { turn } = await startIdleTurn();
      completeTool(
        "CronCreate",
        { cron: "* * * * *", recurring },
        undefined,
        `Scheduled ${recurring ? "recurring job" : "one-shot task"} native-job (* * * * *).`,
      );
      emit({ type: "result", subtype: "success" });
      await turn;
      completeTool("CronDelete", { id: "native-job" }, {});
      emit({ type: "result", subtype: "success" });
      expect(claudeSessionNeedsProcess("audit-idle")).toBe(false);
    },
  );
  it("uses the structured cron ID when a job is deleted", async () => {
    const { turn } = await startIdleTurn();
    completeTool(
      "CronCreate",
      { cron: "* * * * *", recurring: true },
      { id: "cron-1", recurring: true },
    );
    emit({ type: "result", subtype: "success" });
    await turn;
    expect(claudeSessionNeedsProcess("audit-idle")).toBe(true);
    completeTool("CronDelete", { id: "cron-1" }, {});
    emit({ type: "result", subtype: "success" });
    expect(claudeSessionNeedsProcess("audit-idle")).toBe(false);
  });

  it("removes only due one-shot jobs when Claude wakes up", async () => {
    vi.setSystemTime(new Date(2026, 9, 3, 12, 3));
    const { turn } = await startIdleTurn();
    completeTool(
      "CronCreate",
      { cron: "5 12 * * *" },
      { id: "once", recurring: false },
    );
    completeTool(
      "CronCreate",
      { cron: "5 13 * * *" },
      { id: "later", recurring: false },
    );
    emit({ type: "result", subtype: "success" });
    await turn;
    await vi.advanceTimersByTimeAsync(2 * 60_000);
    emit({ type: "system", subtype: "init", session_id: "provider-idle" });
    emit({ type: "result", subtype: "success" });
    expect(claudeSessionNeedsProcess("audit-idle")).toBe(true);
    await vi.advanceTimersByTimeAsync(60 * 60_000);
    emit({ type: "system", subtype: "init", session_id: "provider-idle" });
    emit({ type: "result", subtype: "success" });
    expect(claudeSessionNeedsProcess("audit-idle")).toBe(false);
    await vi.advanceTimersByTimeAsync(300_001);
    expect(killChild).toHaveBeenCalled();
  });

  it("reconciles the scheduled jobs reported by CronList", async () => {
    const { turn } = await startIdleTurn();
    completeTool(
      "CronCreate",
      { cron: "* * * * *", recurring: true },
      { id: "expired", recurring: true },
    );
    completeTool("CronList", {}, { jobs: [] });
    emit({ type: "result", subtype: "success" });
    await turn;
    expect(claudeSessionNeedsProcess("audit-idle")).toBe(false);
  });

  it("releases recurring jobs after their expiry and final-fire jitter", async () => {
    const { turn } = await startIdleTurn();
    completeTool(
      "CronCreate",
      { cron: "* * * * *", recurring: true },
      { id: "recurring", recurring: true },
    );
    emit({ type: "result", subtype: "success" });
    await turn;
    expect(claudeSessionNeedsProcess("audit-idle")).toBe(true);
    vi.setSystemTime(Date.now() + 7 * 86_400_000 + 31 * 60_000);
    expect(claudeSessionNeedsProcess("audit-idle")).toBe(false);
  });
  it.each(["task_notification", "task_updated"])(
    "releases finished ambient work after %s",
    async (subtype) => {
      const turn = sendHarnessTurn({
        harness: "claude",
        sessionId: "audit-idle",
        cwd: "/repo",
        model: "claude:claude-haiku-4-5",
        runtimeMode: "supervised",
        text: "Watch a native task",
        attachments: [],
        onEvent: () => undefined,
      });
      await tick();
      emit({
        type: "control_response",
        response: {
          subtype: "success",
          request_id: "monocode_2",
          response: {},
        },
      });
      emit({ type: "system", subtype: "init", session_id: "provider-idle" });
      await tick();
      emit({
        type: "system",
        subtype: "task_started",
        task_id: "ambient-1",
        ambient: true,
      });
      emit({ type: "result", subtype: "success" });
      await turn;
      expect(claudeSessionNeedsProcess("audit-idle")).toBe(true);
      emit({
        type: "system",
        subtype,
        task_id: "ambient-1",
        ambient: true,
        status: "completed",
        patch: { status: "completed" },
      });
      expect(claudeSessionNeedsProcess("audit-idle")).toBe(false);
      await vi.advanceTimersByTimeAsync(300_001);
      expect(killChild).toHaveBeenCalled();
    },
  );
  it("adds a native wakeup's tokens to the user turn instead of replacing them", async () => {
    const events: HarnessEvent[] = [];
    const turn = sendHarnessTurn({
      harness: "claude",
      sessionId: "audit-idle",
      cwd: "/repo",
      model: "claude:claude-haiku-4-5",
      runtimeMode: "supervised",
      text: "Set up a session reminder",
      attachments: [],
      onEvent: (event) => events.push(event),
    });
    await tick();
    emit({
      type: "control_response",
      response: { subtype: "success", request_id: "monocode_2", response: {} },
    });
    emit({ type: "system", subtype: "init", session_id: "provider-idle" });
    await tick();
    emit({
      type: "result",
      subtype: "success",
      result: "Scheduled",
      usage: { input_tokens: 100, output_tokens: 10 },
    });
    await turn;
    emit({ type: "system", subtype: "init", session_id: "provider-idle" });
    emit({
      type: "stream_event",
      event: {
        type: "content_block_delta",
        index: 0,
        delta: { type: "text_delta", text: "The reminder fired." },
      },
    });
    emit({
      type: "result",
      subtype: "success",
      result: "The reminder fired.",
      usage: { input_tokens: 5, output_tokens: 1 },
    });

    expect(
      events.some((event) => event.type === "turn.started" && event.native),
    ).toBe(true);
    const metrics = events.filter((event) => event.type === "turn.metrics");
    expect(metrics.at(-1)).toMatchObject({
      inputTokens: 105,
      outputTokens: 11,
    });
  });

  it("does not park Claude while an unsolicited native wakeup is generating", async () => {
    const events: HarnessEvent[] = [];
    const turn = sendHarnessTurn({
      harness: "claude",
      sessionId: "audit-idle",
      cwd: "/repo",
      model: "claude:claude-haiku-4-5",
      runtimeMode: "supervised",
      text: "Set up a session reminder",
      attachments: [],
      onEvent: (event) => events.push(event),
    });
    await tick();
    emit({
      type: "control_response",
      response: { subtype: "success", request_id: "monocode_2", response: {} },
    });
    emit({ type: "system", subtype: "init", session_id: "provider-idle" });
    await tick();
    expect(sent.some((record) => record.type === "user")).toBe(true);
    emit({ type: "result", subtype: "success", result: "Scheduled" });
    await turn;
    await vi.advanceTimersByTimeAsync(60_000);
    emit({ type: "system", subtype: "init", session_id: "provider-idle" });
    emit({
      type: "stream_event",
      event: {
        type: "content_block_delta",
        index: 0,
        delta: { type: "text_delta", text: "The scheduled check has started." },
      },
    });
    expect(
      events.some(
        (event) =>
          event.type === "message.delta" &&
          event.text === "The scheduled check has started.",
      ),
    ).toBe(true);
    await vi.advanceTimersByTimeAsync(240_001);
    expect(killChild).not.toHaveBeenCalled();
  });
});
