import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const sent: string[] = [];
let onLine: ((line: string) => void) | undefined;

vi.mock("./child", () => ({
  resolveClaudeBinary: async () => ({ path: "/fake/claude" }),
  spawnChild: async () => undefined,
  killChild: async () => undefined,
  unwatchChild: () => undefined,
  watchChild: (_id: string, line: (l: string) => void) => {
    onLine = line;
  },
  writeChild: async (_id: string, line: string) => {
    sent.push(line);
  },
}));

const {
  compactClaudeContext,
  sendClaudeTurn,
  stopClaudeSession,
  __claudeTestReset,
} = await import("./claude");
import type { HarnessEvent } from "./types";
import type { RuntimeMode, TurnIntent } from "../session";

function parse() {
  return sent.map((line) => JSON.parse(line) as Record<string, unknown>);
}

function emit(rec: Record<string, unknown>) {
  onLine!(JSON.stringify(rec));
}

const waitFor = async (pred: () => boolean, label: string) => {
  for (let i = 0; i < 200; i++) {
    if (pred()) return;
    await new Promise((r) => setTimeout(r, 5));
  }
  throw new Error(
    `timed out waiting for ${label}; sent=${JSON.stringify(parse())}`,
  );
};

async function startTurn(
  sessionId: string,
  options: { runtimeMode?: RuntimeMode; intent?: TurnIntent } = {},
) {
  const events: HarnessEvent[] = [];
  const turn = sendClaudeTurn({
    sessionId,
    cwd: "/repo",
    model: "claude:claude-sonnet-5",
    modelSettings: {},
    runtimeMode: options.runtimeMode ?? "supervised",
    intent: options.intent,
    text: "explore the codebase",
    attachments: [],
    onEvent: (event) => events.push(event),
  });

  await waitFor(
    () =>
      parse().some((m) => {
        const request = m.request as Record<string, unknown> | undefined;
        return request?.subtype === "initialize";
      }),
    "initialize",
  );
  emit({ type: "system", subtype: "init", session_id: "sess_1" });
  emit({
    type: "control_response",
    response: { subtype: "success", request_id: "monocode_1" },
  });
  await waitFor(() => parse().some((m) => m.type === "user"), "user prompt");
  return { events, turn };
}

beforeEach(() => {
  sent.length = 0;
  onLine = undefined;
  __claudeTestReset();
});

afterEach(async () => {
  await stopClaudeSession("s1");
  __claudeTestReset();
});

describe("claude subagents", () => {
  it("stays busy after a parent result while a background subagent is running", async () => {
    const { events, turn } = await startTurn("s1");
    let settled = false;
    void turn.then(() => {
      settled = true;
    });

    emit({
      type: "assistant",
      session_id: "sess_1",
      message: {
        content: [
          {
            type: "tool_use",
            id: "toolu_agent",
            name: "Agent",
            input: {
              description: "Explore the auth module",
              subagent_type: "explore",
            },
          },
        ],
      },
    });
    emit({
      type: "system",
      subtype: "task_started",
      task_id: "t1",
      tool_use_id: "toolu_agent",
      description: "Explore the auth module",
      task_type: "local_agent",
      is_backgrounded: true,
    });
    emit({
      type: "user",
      session_id: "sess_1",
      message: {
        content: [
          {
            type: "tool_result",
            tool_use_id: "toolu_agent",
            content: "Backgrounded",
          },
        ],
      },
    });
    emit({
      type: "result",
      subtype: "success",
      session_id: "sess_1",
    });

    await new Promise((r) => setTimeout(r, 30));
    expect(settled).toBe(false);
    expect(
      events.some(
        (event) =>
          event.type === "tool.started" &&
          event.kind === "agent" &&
          event.title === "Explore the auth module",
      ),
    ).toBe(true);
    expect(events.some((event) => event.type === "message.completed")).toBe(
      false,
    );

    emit({
      type: "system",
      subtype: "task_notification",
      task_id: "t1",
      tool_use_id: "toolu_agent",
      status: "completed",
      summary: "Found the tokens",
    });
    await turn;
    expect(settled).toBe(true);
    expect(events.some((event) => event.type === "message.completed")).toBe(
      true,
    );
  });

  it("does not end the turn on a subagent result", async () => {
    const { events, turn } = await startTurn("s1");
    let settled = false;
    void turn.then(() => {
      settled = true;
    });

    emit({
      type: "assistant",
      session_id: "sess_1",
      message: {
        content: [
          {
            type: "tool_use",
            id: "toolu_agent",
            name: "Agent",
            input: { description: "Explore", subagent_type: "explore" },
          },
        ],
      },
    });
    emit({
      type: "result",
      subtype: "success",
      session_id: "sess_sub",
      parent_tool_use_id: "toolu_agent",
    });

    await new Promise((r) => setTimeout(r, 30));
    expect(settled).toBe(false);
    expect(events.some((event) => event.type === "message.completed")).toBe(
      false,
    );

    emit({ type: "result", subtype: "success", session_id: "sess_1" });
    await turn;
    expect(settled).toBe(true);
  });

  it("keeps the turn open when the Agent result is only a launch receipt", async () => {
    const { events, turn } = await startTurn("s1");
    let settled = false;
    void turn.then(() => {
      settled = true;
    });

    emit({
      type: "assistant",
      session_id: "sess_1",
      message: {
        content: [
          {
            type: "tool_use",
            id: "toolu_agent",
            name: "Agent",
            input: { description: "Explore", subagent_type: "Explore" },
          },
        ],
      },
    });
    // Claude Code 2.1.260 launches agents asynchronously by default and
    // answers the call at once with a receipt; the result comes later.
    emit({
      type: "user",
      session_id: "sess_1",
      message: {
        content: [
          {
            type: "tool_result",
            tool_use_id: "toolu_agent",
            content:
              "Async agent launched successfully. agentId: a1 The agent is working in the background.",
          },
        ],
      },
    });
    emit({ type: "result", subtype: "success", session_id: "sess_1" });
    await new Promise((r) => setTimeout(r, 20));
    expect(settled).toBe(false);
    expect(
      events.some(
        (event) =>
          event.type === "tool.updated" &&
          event.callId === "toolu_agent" &&
          event.status === "completed",
      ),
    ).toBe(false);

    emit({
      type: "system",
      subtype: "task_started",
      session_id: "sess_1",
      task_id: "task_1",
      tool_use_id: "toolu_agent",
      description: "Explore",
      task_type: "local_agent",
      is_backgrounded: false,
    });
    emit({
      type: "system",
      subtype: "task_notification",
      session_id: "sess_1",
      task_id: "task_1",
      tool_use_id: "toolu_agent",
      status: "completed",
      summary: "Found it.",
    });
    await turn;
    expect(settled).toBe(true);
    expect(
      events.some(
        (event) =>
          event.type === "tool.updated" &&
          event.callId === "toolu_agent" &&
          event.status === "completed" &&
          event.detail === "Found it.",
      ),
    ).toBe(true);
  });

  it("does not dump subagent assistant text into the parent transcript", async () => {
    const { events, turn } = await startTurn("s1");
    emit({
      type: "assistant",
      session_id: "sess_1",
      message: {
        content: [
          {
            type: "tool_use",
            id: "toolu_agent",
            name: "Agent",
            input: { description: "Explore", subagent_type: "explore" },
          },
        ],
      },
    });
    emit({
      type: "assistant",
      parent_tool_use_id: "toolu_agent",
      message: { content: [{ type: "text", text: "I will grep for tokens" }] },
    });
    emit({ type: "result", subtype: "success", session_id: "sess_1" });
    await turn;
    expect(
      events.some(
        (event) =>
          event.type === "message.delta" &&
          event.text.includes("I will grep for tokens"),
      ),
    ).toBe(false);
  });
});

describe("claude plan permissions", () => {
  it("answers residual plan-mode permissions without prompting the user", async () => {
    const { events, turn } = await startTurn("s1", {
      runtimeMode: "auto",
      intent: "plan",
    });

    emit({
      type: "control_request",
      request_id: "read_1",
      request: {
        subtype: "can_use_tool",
        tool_name: "Read",
        input: { file_path: "/repo/src/App.tsx" },
      },
    });
    emit({
      type: "control_request",
      request_id: "write_1",
      request: {
        subtype: "can_use_tool",
        tool_name: "Write",
        input: { file_path: "/repo/src/new.ts" },
      },
    });

    await waitFor(
      () =>
        parse().filter((message) => message.type === "control_response")
          .length >= 2,
      "plan permission responses",
    );
    const responses = parse().filter(
      (message) => message.type === "control_response",
    );
    const read = responses.find(
      (message) =>
        (message.response as Record<string, unknown>)?.request_id === "read_1",
    );
    const write = responses.find(
      (message) =>
        (message.response as Record<string, unknown>)?.request_id === "write_1",
    );
    expect(
      (
        (read?.response as Record<string, unknown>)?.response as Record<
          string,
          unknown
        >
      )?.behavior,
    ).toBe("allow");
    expect(
      (
        (write?.response as Record<string, unknown>)?.response as Record<
          string,
          unknown
        >
      )?.behavior,
    ).toBe("deny");
    expect(events.some((event) => event.type === "approval.requested")).toBe(
      false,
    );

    emit({ type: "result", subtype: "success", session_id: "sess_1" });
    await turn;
  });
});

describe("claude manual compaction", () => {
  it("runs the built-in command and requires a compact boundary", async () => {
    const { turn } = await startTurn("s1");
    emit({ type: "result", subtype: "success", session_id: "sess_1" });
    await turn;
    sent.length = 0;

    const events: HarnessEvent[] = [];
    const compact = compactClaudeContext({
      sessionId: "s1",
      cwd: "/repo",
      model: "claude:claude-sonnet-5",
      runtimeMode: "supervised",
      onEvent: (event) => events.push(event),
    });
    await waitFor(
      () => parse().some((message) => message.type === "user"),
      "compact command",
    );
    expect(parse().find((message) => message.type === "user")).toMatchObject({
      message: { content: [{ type: "text", text: "/compact" }] },
    });

    emit({
      type: "assistant",
      session_id: "sess_1",
      message: { content: [{ type: "text", text: "not transcript output" }] },
    });
    emit({
      type: "system",
      subtype: "compact_boundary",
      session_id: "sess_1",
    });
    emit({ type: "result", subtype: "success", session_id: "sess_1" });
    await compact;

    expect(events).toContainEqual({
      type: "status",
      text: "Compacted context",
    });
    expect(events.some((event) => event.type === "message.delta")).toBe(false);
  });
});

describe("claude subagent transcripts", () => {
  it("nests a subagent's text and tool calls under its Agent call", async () => {
    const { events, turn } = await startTurn("s1");
    emit({
      type: "assistant",
      session_id: "sess_1",
      message: {
        content: [
          {
            type: "tool_use",
            id: "toolu_agent",
            name: "Agent",
            input: {
              description: "Explore",
              subagent_type: "explore",
              prompt: "Find the token refresh path",
            },
          },
        ],
      },
    });
    emit({
      type: "assistant",
      parent_tool_use_id: "toolu_agent",
      message: {
        model: "claude-haiku-4-5-20251001",
        content: [
          { type: "text", text: "I will grep for tokens" },
          {
            type: "tool_use",
            id: "toolu_grep",
            name: "Grep",
            input: { pattern: "refreshToken" },
          },
        ],
      },
    });
    emit({
      type: "user",
      parent_tool_use_id: "toolu_agent",
      message: {
        content: [
          {
            type: "tool_result",
            tool_use_id: "toolu_grep",
            content: "src/auth.ts:12",
          },
        ],
      },
    });
    emit({ type: "result", subtype: "success", session_id: "sess_1" });
    await turn;

    expect(events).toContainEqual({
      type: "subagent.updated",
      callId: "toolu_agent",
      agentType: "explore",
      prompt: "Find the token refresh path",
    });
    expect(events).toContainEqual({
      type: "subagent.updated",
      callId: "toolu_agent",
      model: "claude-haiku-4-5-20251001",
    });
    const nested = events.filter(
      (event) =>
        event.type === "subagent.event" && event.callId === "toolu_agent",
    );
    const inner = nested.map((event) =>
      event.type === "subagent.event" ? event.event : event,
    );
    expect(inner).toContainEqual({
      type: "message.delta",
      text: "I will grep for tokens",
    });
    expect(
      inner.some(
        (event) =>
          event.type === "tool.started" && event.callId === "toolu_grep",
      ),
    ).toBe(true);
    expect(
      inner.some(
        (event) =>
          event.type === "tool.updated" &&
          event.callId === "toolu_grep" &&
          event.status === "completed",
      ),
    ).toBe(true);
    // The parent transcript still only sees the Agent row and its latest step.
    expect(
      events.some(
        (event) =>
          event.type === "tool.started" && event.callId === "toolu_grep",
      ),
    ).toBe(false);
    expect(
      events.some(
        (event) =>
          event.type === "tool.updated" &&
          event.callId === "toolu_agent" &&
          event.detail === "Find refreshToken",
      ),
    ).toBe(true);
  });

  it("sends each piece of subagent metadata once and drops lines after completion", async () => {
    const { events, turn } = await startTurn("s1");
    emit({
      type: "assistant",
      session_id: "sess_1",
      message: {
        content: [
          {
            type: "tool_use",
            id: "toolu_agent",
            name: "Agent",
            input: { description: "Explore", subagent_type: "explore" },
          },
        ],
      },
    });
    for (const text of ["one", "two"]) {
      emit({
        type: "assistant",
        parent_tool_use_id: "toolu_agent",
        message: {
          model: "claude-haiku-4-5-20251001",
          content: [{ type: "text", text }],
        },
      });
    }
    emit({
      type: "user",
      session_id: "sess_1",
      message: {
        content: [
          { type: "tool_result", tool_use_id: "toolu_agent", content: "done" },
        ],
      },
    });
    emit({
      type: "assistant",
      parent_tool_use_id: "toolu_agent",
      message: { content: [{ type: "text", text: "late" }] },
    });
    emit({ type: "result", subtype: "success", session_id: "sess_1" });
    await turn;

    const updates = events.filter((event) => event.type === "subagent.updated");
    expect(updates).toEqual([
      { type: "subagent.updated", callId: "toolu_agent", agentType: "explore" },
      {
        type: "subagent.updated",
        callId: "toolu_agent",
        model: "claude-haiku-4-5-20251001",
      },
    ]);
    const nestedText = events.flatMap((event) =>
      event.type === "subagent.event" && event.event.type === "message.delta"
        ? [event.event.text]
        : [],
    );
    expect(nestedText).toEqual(["one", "two"]);
  });

  it("does not mint a stray row when the task list lands before task_started", async () => {
    const { events, turn } = await startTurn("s1");
    emit({
      type: "assistant",
      session_id: "sess_1",
      message: {
        content: [
          {
            type: "tool_use",
            id: "toolu_agent",
            name: "Agent",
            input: {
              description: "Find SQLite session persistence file",
              subagent_type: "Explore",
              prompt: "Find it",
              run_in_background: true,
            },
          },
        ],
      },
    });
    emit({
      type: "system",
      subtype: "background_tasks_changed",
      tasks: [
        {
          task_id: "t1",
          task_type: "local_agent",
          description: "Find SQLite session persistence file",
        },
      ],
    });
    emit({
      type: "system",
      subtype: "task_started",
      task_id: "t1",
      tool_use_id: "toolu_agent",
      description: "Find SQLite session persistence file",
      subagent_type: "Explore",
      prompt: "Find it",
      is_backgrounded: true,
      task_type: "local_agent",
    });
    emit({
      type: "assistant",
      parent_tool_use_id: "toolu_agent",
      message: { content: [{ type: "text", text: "Searching" }] },
    });
    emit({ type: "system", subtype: "background_tasks_changed", tasks: [] });
    emit({
      type: "system",
      subtype: "task_notification",
      task_id: "t1",
      tool_use_id: "toolu_agent",
      status: "completed",
      summary: "src-tauri/src/session_store.rs",
    });
    emit({ type: "result", subtype: "success", session_id: "sess_1" });
    await turn;

    const started = events.filter((event) => event.type === "tool.started");
    expect(
      started.map((event) => event.type === "tool.started" && event.callId),
    ).toEqual(["toolu_agent"]);
    expect(
      events.some(
        (event) =>
          event.type === "tool.updated" &&
          event.callId === "toolu_agent" &&
          event.status === "completed",
      ),
    ).toBe(true);
    expect(
      events.some(
        (event) =>
          event.type === "subagent.event" &&
          event.callId === "toolu_agent" &&
          event.event.type === "message.delta",
      ),
    ).toBe(true);
  });

  it("binds a task that never announced its call to the Agent row with its brief", async () => {
    const { events, turn } = await startTurn("s1");
    emit({
      type: "assistant",
      session_id: "sess_1",
      message: {
        content: [
          {
            type: "tool_use",
            id: "toolu_agent",
            name: "Agent",
            input: { description: "Audit the tests" },
          },
        ],
      },
    });
    emit({
      type: "system",
      subtype: "background_tasks_changed",
      tasks: [
        {
          task_id: "t9",
          task_type: "local_agent",
          description: "Audit the tests",
        },
      ],
    });
    emit({ type: "result", subtype: "success", session_id: "sess_1" });
    emit({ type: "system", subtype: "background_tasks_changed", tasks: [] });
    await turn;
    const started = events.filter((event) => event.type === "tool.started");
    expect(started).toHaveLength(1);
    expect(
      events.some(
        (event) =>
          event.type === "tool.updated" &&
          event.callId === "toolu_agent" &&
          event.status === "completed",
      ),
    ).toBe(true);
  });

  it("keeps the brief as the row title while progress descriptions drift", async () => {
    const { events, turn } = await startTurn("s1");
    emit({
      type: "assistant",
      session_id: "sess_1",
      message: {
        content: [
          {
            type: "tool_use",
            id: "toolu_agent",
            name: "Agent",
            input: { description: "Find session persistence code" },
          },
        ],
      },
    });
    emit({
      type: "system",
      subtype: "task_started",
      task_id: "t1",
      tool_use_id: "toolu_agent",
      description: "Find session persistence code",
      task_type: "local_agent",
      is_backgrounded: true,
    });
    emit({
      type: "system",
      subtype: "task_progress",
      task_id: "t1",
      tool_use_id: "toolu_agent",
      description: 'Running grep -rln "sessionCache"',
    });
    emit({
      type: "system",
      subtype: "task_notification",
      task_id: "t1",
      tool_use_id: "toolu_agent",
      status: "completed",
      summary: "Found it",
    });
    emit({ type: "result", subtype: "success", session_id: "sess_1" });
    await turn;
    const updates = events.filter(
      (event) =>
        event.type === "tool.updated" && event.callId === "toolu_agent",
    );
    expect(updates.length).toBeGreaterThan(0);
    for (const update of updates) {
      expect(update.type === "tool.updated" && update.title).toBe(
        "Find session persistence code",
      );
    }
    expect(
      updates.some(
        (event) =>
          event.type === "tool.updated" &&
          event.detail === 'Running grep -rln "sessionCache"',
      ),
    ).toBe(true);
  });

  it("drops subagent lines whose parent is not a known Agent call", async () => {
    const { events, turn } = await startTurn("s1");
    emit({
      type: "assistant",
      parent_tool_use_id: "toolu_unknown",
      message: { content: [{ type: "text", text: "orphan" }] },
    });
    emit({ type: "result", subtype: "success", session_id: "sess_1" });
    await turn;
    expect(events.some((event) => event.type === "subagent.event")).toBe(false);
    expect(
      events.some(
        (event) => event.type === "message.delta" && event.text === "orphan",
      ),
    ).toBe(false);
  });
});
