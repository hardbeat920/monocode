import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { applyHarnessEvent } from "../../core/apply";
import { newSession } from "../../../../features/sessions/model/session";
import type { HarnessEvent } from "../../core/types";

const sent: Record<string, unknown>[] = [];
let onLine: ((line: string) => void) | undefined;
vi.mock("../../core/child", () => ({
  resolveClaudeBinary: async () => ({ path: "/fake/claude" }),
  spawnChild: async () => undefined,
  killChild: async () => undefined,
  unwatchChild: () => undefined,
  watchChild: (_id: string, line: (line: string) => void) => {
    onLine = line;
  },
  writeChild: async (_id: string, line: string) => {
    sent.push(JSON.parse(line));
  },
}));
const { sendClaudeTurn, stopClaudeSession, __claudeTestReset } =
  await import("./claude");

function emit(rec: Record<string, unknown>) {
  onLine!(JSON.stringify(rec));
}
async function waitFor(pred: () => boolean) {
  for (let i = 0; i < 100; i++) {
    if (pred()) return;
    await new Promise((resolve) => setTimeout(resolve, 2));
  }
  throw new Error("Probe setup timed out");
}
async function start() {
  const events: HarnessEvent[] = [];
  const turn = sendClaudeTurn({
    sessionId: "audit",
    cwd: "/repo",
    model: "claude:claude-sonnet-5",
    modelSettings: {},
    runtimeMode: "supervised",
    text: "Audit",
    attachments: [],
    onEvent: (event) => {
      events.push(event);
    },
  });
  await waitFor(() => sent.some((rec) => rec.type === "control_request"));
  emit({
    type: "control_response",
    response: { subtype: "success", request_id: "monocode_2", response: {} },
  });
  emit({ type: "system", subtype: "init", session_id: "provider-audit" });
  await waitFor(() => sent.some((rec) => rec.type === "user"));
  return { events, turn };
}
function render(events: HarnessEvent[]) {
  return events.reduce(
    applyHarnessEvent,
    newSession("claude", "/repo", "claude:claude-sonnet-5"),
  );
}
function delta(text: string) {
  emit({
    type: "stream_event",
    event: {
      type: "content_block_delta",
      index: 0,
      delta: { type: "text_delta", text },
    },
  });
}
function result(extra: Record<string, unknown> = {}) {
  emit({ type: "result", subtype: "success", ...extra });
}
function startAgent() {
  emit({
    type: "assistant",
    message: {
      id: "parent-message",
      content: [
        {
          type: "tool_use",
          id: "agent-call",
          name: "Agent",
          input: { description: "Review" },
        },
      ],
    },
  });
}

beforeEach(() => {
  sent.length = 0;
  onLine = undefined;
  __claudeTestReset();
});
afterEach(async () => {
  await stopClaudeSession("audit");
  __claudeTestReset();
});

describe("review probes for the official v0.7.0 transcript", () => {
  it("preserves repeated incremental text chunks", async () => {
    const { events, turn } = await start();
    delta("ha");
    delta("ha");
    delta("!");
    emit({
      type: "assistant",
      message: { id: "answer", content: [{ type: "text", text: "haha!" }] },
    });
    result();
    await turn;
    expect(
      render(events)
        .blocks.filter((block) => block.role === "assistant")
        .map((block) => block.text)
        .join(""),
    ).toBe("haha!");
  });

  it("does not replace the final request's context with aggregate turn usage", async () => {
    const { events, turn } = await start();
    emit({
      type: "assistant",
      message: {
        id: "last-request",
        content: [{ type: "text", text: "Done" }],
        usage: {
          input_tokens: 5,
          cache_read_input_tokens: 40_000,
          output_tokens: 1,
        },
      },
    });
    result({
      usage: {
        input_tokens: 20,
        cache_read_input_tokens: 100_000,
        output_tokens: 500,
      },
      modelUsage: { "claude-sonnet-5": { contextWindow: 200_000 } },
    });
    await turn;
    expect(render(events).context?.used).toBeLessThan(50_000);
  });

  it("does not use a subagent's larger context window for the main model", async () => {
    const { events, turn } = await start();
    result({
      usage: { input_tokens: 10, output_tokens: 5 },
      modelUsage: {
        "claude-sonnet-5": { contextWindow: 200_000 },
        "claude-opus-5[1m]": { contextWindow: 1_000_000 },
      },
    });
    await turn;
    expect(render(events).context?.window).toBe(200_000);
  });

  it("keeps successful subagent tool output available for inspection", async () => {
    const { events, turn } = await start();
    startAgent();
    emit({
      type: "assistant",
      parent_tool_use_id: "agent-call",
      message: {
        id: "sub-message",
        content: [
          {
            type: "tool_use",
            id: "shell-call",
            name: "Bash",
            input: { command: "npm test" },
          },
        ],
      },
    });
    emit({
      type: "user",
      parent_tool_use_id: "agent-call",
      message: {
        content: [
          {
            type: "tool_result",
            tool_use_id: "shell-call",
            content: "42 tests passed",
          },
        ],
      },
    });
    result();
    await turn;
    const step = render(events)
      .blocks.find((block) => block.tool?.callId === "agent-call")
      ?.agentRun?.steps.find((step) => step.id === "shell-call");
    expect(JSON.stringify(step)).toContain("42 tests passed");
  });

  it("keeps all subagent text blocks that share one API message id", async () => {
    const { events, turn } = await start();
    startAgent();
    for (const text of ["First observation.", "Second observation."]) {
      emit({
        type: "assistant",
        parent_tool_use_id: "agent-call",
        message: { id: "sub-message", content: [{ type: "text", text }] },
      });
    }
    result();
    await turn;
    const text = render(events)
      .blocks.find((block) => block.tool?.callId === "agent-call")
      ?.agentRun?.steps.map((step) => step.text)
      .join("\n");
    expect(text).toContain("First observation.");
    expect(text).toContain("Second observation.");
  });

  it("counts the task follow-up work in the same MonoCode user turn", async () => {
    const { events, turn } = await start();
    emit({
      type: "system",
      subtype: "task_started",
      task_id: "background-shell",
      task_type: "local_bash",
      description: "Run tests",
    });
    result({
      usage: {
        input_tokens: 50,
        cache_read_input_tokens: 5000,
        output_tokens: 1000,
      },
    });
    emit({
      type: "system",
      subtype: "task_notification",
      task_id: "background-shell",
      status: "completed",
      summary: "Tests passed",
    });
    emit({ type: "system", subtype: "init", session_id: "provider-audit" });
    delta("The tests passed.");
    emit({
      type: "assistant",
      message: { content: [{ type: "text", text: "The tests passed." }] },
    });
    result({
      usage: { input_tokens: 5, cache_read_input_tokens: 0, output_tokens: 20 },
    });
    await turn;
    const session = newSession("claude", "/repo", "claude:claude-sonnet-5");
    session.blocks = [{ id: "user-turn", role: "user", text: "Run tests" }];
    const actual = events.reduce(applyHarnessEvent, session).blocks[0]
      .turnMetrics;
    expect(actual?.inputTokens).toBe(55);
    expect(actual?.outputTokens).toBe(1020);
  });
});
