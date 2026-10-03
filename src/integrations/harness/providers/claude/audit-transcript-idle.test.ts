import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { HarnessEvent } from "../../core/types";

const sent: Record<string, unknown>[] = [];
let onLine: ((line: string) => void) | undefined;
const killChild = vi.fn(async () => undefined);
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
const { __claudeTestReset, stopClaudeSession } = await import("./claude");
const { claudeAdapter } = await import("./claudeAdapter");
const { registerHarness, sendHarnessTurn, resetHarnessIdlePark } =
  await import("../../core/registry");
const tick = async () => {
  for (let i = 0; i < 30; i++) await Promise.resolve();
};
const emit = (record: Record<string, unknown>) =>
  onLine!(JSON.stringify(record));

beforeEach(() => {
  vi.useFakeTimers();
  sent.length = 0;
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
