import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { HarnessEvent } from "../../core/types";

const spawned: string[][] = [];
const sent: Record<string, unknown>[] = [];
let onLine: ((line: string) => void) | undefined;
vi.mock("../../core/child", () => ({
  resolveClaudeBinary: async () => ({ path: "/fake/claude" }),
  spawnChild: async (_id: string, _path: string, args: string[]) => {
    spawned.push(args);
    onLine!(JSON.stringify({ type: "system", subtype: "init" }));
  },
  killChild: async () => undefined,
  unwatchChild: () => undefined,
  watchChild: (_id: string, line: (line: string) => void) => {
    onLine = line;
  },
  writeChild: async (_id: string, line: string) => {
    const record = JSON.parse(line);
    sent.push(record);
    if (record.request?.subtype === "initialize")
      onLine!(
        JSON.stringify({
          type: "control_response",
          response: {
            subtype: "success",
            request_id: record.request_id,
            response: {},
          },
        }),
      );
  },
}));
const { runClaudeTextPrompt, stopClaudeTextPrompt } =
  await import("./claudeText");
function emit(rec: Record<string, unknown>) {
  onLine!(JSON.stringify(rec));
}
async function waitForPrompt() {
  for (let i = 0; i < 100; i++) {
    if (sent.some((rec) => rec.type === "user")) return;
    await new Promise((resolve) => setTimeout(resolve, 2));
  }
  throw new Error("Probe setup timed out");
}
beforeEach(() => {
  spawned.length = 0;
  sent.length = 0;
});
afterEach(async () => {
  await stopClaudeTextPrompt();
});

describe("review probes for the official v0.7.0 Claude text runner", () => {
  it("restarts a warmed helper to apply explicit Thinking Off", async () => {
    const first = runClaudeTextPrompt({ cwd: "/repo", prompt: "First" });
    await waitForPrompt();
    emit({
      type: "assistant",
      message: { content: [{ type: "text", text: "First" }] },
    });
    emit({ type: "result", subtype: "success" });
    await first;
    sent.length = 0;
    const second = runClaudeTextPrompt({
      cwd: "/repo",
      prompt: "Second",
      modelSettings: { thinking: "false" },
    });
    await waitForPrompt();
    expect(spawned).toHaveLength(2);
    const args = spawned[1];
    expect(JSON.parse(args[args.indexOf("--settings") + 1])).toMatchObject({
      alwaysThinkingEnabled: false,
    });
    emit({
      type: "assistant",
      message: { content: [{ type: "text", text: "Second" }] },
    });
    emit({ type: "result", subtype: "success" });
    await second;
  });
  it("preserves repeated incremental chunks in titles and other helper outputs", async () => {
    const prompt = runClaudeTextPrompt({ cwd: "/repo", prompt: "Say haha!" });
    await waitForPrompt();
    for (const text of ["ha", "ha", "!"]) {
      emit({
        type: "stream_event",
        event: {
          type: "content_block_delta",
          index: 0,
          delta: { type: "text_delta", text },
        },
      });
    }
    emit({
      type: "assistant",
      message: { content: [{ type: "text", text: "haha!" }] },
    });
    emit({ type: "result", subtype: "success" });
    expect(await prompt).toBe("haha!");
  });

  it("allows a read-only side question to inspect a file and then answer", async () => {
    const prompt = runClaudeTextPrompt({
      cwd: "/repo",
      intent: "plan",
      prompt: "Read README.md and explain its setup.",
    });
    await waitForPrompt();
    emit({
      type: "assistant",
      message: { content: [{ type: "text", text: "Answer" }] },
    });
    emit({ type: "result", subtype: "success" });
    await prompt;
    const args = spawned[0];
    const limit = args.indexOf("--max-turns");
    expect(limit < 0 || Number(args[limit + 1]) >= 2).toBe(true);
  });

  it("publishes the completed result of a helper tool", async () => {
    const events: HarnessEvent[] = [];
    const prompt = runClaudeTextPrompt({
      cwd: "/repo",
      intent: "plan",
      prompt: "Inspect README.md",
      onEvent: (event) => events.push(event),
    });
    await waitForPrompt();
    emit({
      type: "stream_event",
      event: {
        type: "content_block_start",
        index: 0,
        content_block: {
          type: "tool_use",
          id: "read-call",
          name: "Read",
          input: { file_path: "/repo/README.md" },
        },
      },
    });
    emit({
      type: "user",
      message: {
        content: [
          {
            type: "tool_result",
            tool_use_id: "read-call",
            content: "Readme contents",
          },
        ],
      },
    });
    emit({
      type: "assistant",
      message: { content: [{ type: "text", text: "Answer" }] },
    });
    emit({ type: "result", subtype: "success" });
    await prompt;
    expect(
      events.some(
        (event) =>
          event.type === "tool.updated" &&
          event.callId === "read-call" &&
          event.status === "completed",
      ),
    ).toBe(true);
  });
});
