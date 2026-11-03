import { afterEach, describe, expect, it, vi } from "vitest";
import type { HarnessEvent } from "../../core/types";

const sent: string[] = [];
const spawned: string[][] = [];
let onLine: ((line: string) => void) | undefined;

vi.mock("../../core/child", () => ({
  resolveClaudeBinary: async () => ({ path: "/fake/claude" }),
  spawnChild: async (_id: string, _path: string, args: string[]) => {
    spawned.push(args);
  },
  killChild: async () => undefined,
  unwatchChild: () => undefined,
  watchChild: (_id: string, line: (l: string) => void) => {
    onLine = line;
  },
  writeChild: async (_id: string, line: string) => {
    sent.push(line);
  },
}));

const { runClaudeTextPrompt, stopClaudeTextPrompt } = await import(
  "./claudeText"
);

function emit(rec: Record<string, unknown>) {
  onLine!(JSON.stringify(rec));
}

async function waitFor(pred: () => boolean, label: string) {
  for (let i = 0; i < 200; i++) {
    if (pred()) return;
    await new Promise((r) => setTimeout(r, 5));
  }
  throw new Error(`timed out waiting for ${label}`);
}

async function startSideAnswer(timeoutMs?: number) {
  sent.length = 0;
  spawned.length = 0;
  onLine = undefined;
  const events: HarnessEvent[] = [];
  const answer = runClaudeTextPrompt({
    cwd: "/repo",
    model: "claude-haiku-4-5",
    intent: "plan",
    prompt: "where is auth?",
    timeoutMs,
    onEvent: (event) => events.push(event),
  });
  await waitFor(() => onLine !== undefined && spawned.length > 0, "spawn");
  emit({ type: "system", subtype: "init" });
  await waitFor(() => sent.length > 0, "prompt");
  return { events, answer };
}

function emitText(text: string) {
  emit({
    type: "stream_event",
    event: {
      type: "content_block_delta",
      index: 0,
      delta: { type: "text_delta", text },
    },
  });
  emit({ type: "assistant", message: { content: [{ type: "text", text }] } });
}

function emitGrep(id: string) {
  emit({
    type: "stream_event",
    event: {
      type: "content_block_start",
      index: 1,
      content_block: { type: "tool_use", id, name: "Grep", input: {} },
    },
  });
  emit({
    type: "assistant",
    message: {
      content: [
        { type: "tool_use", id, name: "Grep", input: { pattern: "auth" } },
      ],
    },
  });
  emit({
    type: "user",
    message: {
      content: [
        { type: "tool_result", tool_use_id: id, content: "src/auth.ts" },
      ],
    },
  });
}

afterEach(async () => {
  await stopClaudeTextPrompt();
});

describe("Claude side answers", () => {
  it("bounds read-only answers instead of capping them at one turn", async () => {
    const { answer } = await startSideAnswer();
    emitText("Done.");
    emit({ type: "result", subtype: "success" });
    await answer;
    const args = spawned[0];
    expect(args[args.indexOf("--max-turns") + 1]).toBe("8");
  });

  it("keeps each message once when an answer spans a tool call", async () => {
    const { events, answer } = await startSideAnswer();
    emitText("Let me check.");
    emitGrep("toolu_1");
    emitText("Auth is in src/auth.ts.");
    emit({ type: "result", subtype: "success" });

    expect(await answer).toBe("Let me check.\n\nAuth is in src/auth.ts.");
    const deltas = events
      .filter((e) => e.type === "message.delta")
      .map((e) => (e as { text: string }).text);
    expect(deltas).toEqual(["Let me check.", "Auth is in src/auth.ts."]);
    expect(events.filter((e) => e.type === "message.completed")).toHaveLength(
      1,
    );
  });

  it("marks tool calls finished when their result arrives", async () => {
    const { events, answer } = await startSideAnswer();
    emitGrep("toolu_1");
    emitText("Found it.");
    emit({ type: "result", subtype: "success" });
    await answer;
    expect(events).toContainEqual(
      expect.objectContaining({
        type: "tool.updated",
        callId: "toolu_1",
        status: "completed",
      }),
    );
  });

  it("only times out when Claude goes quiet", async () => {
    const { answer } = await startSideAnswer(50);
    for (let i = 0; i < 4; i++) {
      await new Promise((r) => setTimeout(r, 30));
      emitGrep(`toolu_${i}`);
    }
    emitText("Still answered.");
    emit({ type: "result", subtype: "success" });
    expect(await answer).toBe("Still answered.");

    const stalled = await startSideAnswer(50);
    await expect(stalled.answer).rejects.toThrow(
      "Claude text generation timed out",
    );
  });

  it("explains when the turn limit cuts the answer off", async () => {
    const { answer } = await startSideAnswer();
    emitGrep("toolu_1");
    emit({ type: "result", subtype: "error_max_turns" });
    await expect(answer).rejects.toThrow("too many tool steps");
  });
});
