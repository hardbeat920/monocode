import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const sent: string[] = [];
let onLine: ((line: string) => void) | undefined;
let onExit: ((code: number) => void) | undefined;

vi.mock("./child", () => ({
  resolveCursorBinary: async () => ({ path: "/fake/cursor-agent" }),
  spawnChild: async () => undefined,
  killChild: async () => undefined,
  unwatchChild: () => undefined,
  watchChild: (
    _id: string,
    line: (value: string) => void,
    exit: (code: number) => void,
  ) => {
    onLine = line;
    onExit = exit;
  },
  writeChild: async (_id: string, line: string) => {
    sent.push(line);
  },
}));

const stores = vi.hoisted(() => ({ tools: vi.fn(), subagents: vi.fn() }));
vi.mock("./cursorStore", () => ({
  readStoredCursorToolCalls: stores.tools,
  readStoredCursorSubagentRuns: stores.subagents,
}));

const {
  sendCursorTurn,
  cancelCursorTurn,
  stopCursorSession,
  __cursorTestReset,
} = await import("./cursor");
import type { HarnessEvent } from "./types";
import { newSession } from "../session";
import { applyHarnessEvent } from "./apply";

function parse() {
  return sent.map((line) => JSON.parse(line) as Record<string, unknown>);
}

function reply(id: number, result: unknown) {
  onLine!(JSON.stringify({ jsonrpc: "2.0", id, result }));
}

function notify(method: string, params: unknown) {
  onLine!(JSON.stringify({ jsonrpc: "2.0", method, params }));
}

function request(id: number, method: string, params: unknown) {
  onLine!(JSON.stringify({ jsonrpc: "2.0", id, method, params }));
}

const waitFor = async (pred: () => boolean, label: string) => {
  for (let i = 0; i < 200; i++) {
    if (pred()) return;
    await new Promise((resolve) => setTimeout(resolve, 5));
  }
  throw new Error(
    `timed out waiting for ${label}; sent=${JSON.stringify(parse())}`,
  );
};

function outboundRequest(method: string) {
  return parse().find((message) => message.method === method);
}

async function startTurn(sessionId: string) {
  const events: HarnessEvent[] = [];
  const turn = sendCursorTurn({
    sessionId,
    cwd: "/repo",
    model: "cursor:composer-2.5",
    modelSettings: {},
    runtimeMode: "supervised",
    text: "explore the codebase",
    attachments: [],
    onEvent: (event) => events.push(event),
  });

  await waitFor(() => !!outboundRequest("initialize"), "initialize");
  reply(outboundRequest("initialize")!.id as number, {});

  await waitFor(() => !!outboundRequest("authenticate"), "authenticate");
  reply(outboundRequest("authenticate")!.id as number, {});

  await waitFor(() => !!outboundRequest("session/new"), "session/new");
  reply(outboundRequest("session/new")!.id as number, {
    sessionId: "cursor_1",
    configOptions: [
      {
        id: "model",
        category: "model",
        currentValue: "composer-2.5",
      },
    ],
  });

  await waitFor(() => !!outboundRequest("session/prompt"), "session/prompt");
  const promptId = outboundRequest("session/prompt")!.id as number;
  return { events, promptId, turn };
}

function emitAgentStart() {
  notify("session/update", {
    sessionId: "cursor_1",
    update: {
      sessionUpdate: "tool_call",
      toolCallId: "call_agent",
      title: "Task: Explore auth",
      kind: "other",
      status: "pending",
      rawInput: {
        _toolName: "task",
        description: "Explore auth",
        subagentType: "explore",
      },
    },
  });
}

function agentEvents(events: HarnessEvent[]) {
  return events.filter(
    (event) => event.type === "tool.updated" && event.callId === "call_agent",
  );
}

beforeEach(() => {
  stores.tools.mockReset().mockResolvedValue([]);
  stores.subagents.mockReset().mockResolvedValue([]);
  sent.length = 0;
  onLine = undefined;
  __cursorTestReset();
});

afterEach(async () => {
  await stopCursorSession("cursor-live");
  __cursorTestReset();
});

describe("cursor background subagents", () => {
  it("recovers native child steps without parent-attributed ACP events and enriches foreground names", async () => {
    const run = {
      agentId: "child_1",
      toolCallId: "call_agent",
      revision: "12",
      prompt:
        "Perform a read-only, defect-first code review of ACP routing in /repo.",
      steps: [
        { id: "child_1:blob:0", kind: "message", text: "Checking routing." },
        {
          id: "child_1:tool:read",
          kind: "tool",
          text: "",
          toolName: "Read",
          args: { path: "/repo/acp.ts" },
          status: "completed",
          output: "export const route = true;",
        },
      ],
    };
    stores.subagents.mockResolvedValue([run]);
    const { events, promptId, turn } = await startTurn("cursor-live");
    notify("session/update", {
      sessionId: "cursor_1",
      update: {
        sessionUpdate: "tool_call",
        toolCallId: "call_agent",
        title: "Task: Subagent task",
        kind: "other",
        status: "pending",
        rawInput: { _toolName: "task" },
      },
    });
    expect(agentEvents(events).at(-1)).toMatchObject({ title: "Subagent" });
    await waitFor(
      () => events.some((event) => event.type === "agent.step"),
      "stored child steps",
    );
    expect(stores.subagents).toHaveBeenCalledWith(
      "cursor_1",
      ["call_agent"],
      expect.any(Object),
    );
    notify("session/update", {
      sessionId: "cursor_1",
      update: {
        sessionUpdate: "tool_call_update",
        toolCallId: "call_agent",
        status: "completed",
      },
    });
    request(101, "cursor/task", {
      toolCallId: "call_agent",
      agentId: "child_1",
      description: "Review ACP routing",
      durationMs: 25,
    });
    reply(promptId, { stopReason: "end_turn" });
    await turn;
    const session = events.reduce(
      applyHarnessEvent,
      newSession("cursor", "/repo"),
    );
    const row = session.blocks.find(
      (block) => block.tool?.callId === "call_agent",
    )!;
    expect(row.text).toBe("Review ACP routing");
    expect(row.tool?.status).toBe("completed");
    expect(row.agentRun?.name).toBe("Review ACP routing");
    expect(row.agentRun?.steps).toHaveLength(2);
    expect(row.agentRun?.steps[1]).toMatchObject({
      toolKind: "read",
      status: "completed",
      preview: { path: "/repo/acp.ts" },
    });
    expect(
      session.blocks.filter((block) => block.role === "assistant"),
    ).toEqual([]);
    expect(stores.subagents.mock.calls.at(-1)?.[2]).toEqual({ child_1: "12" });
  });

  it("keeps a task description received before its placeholder tool row", async () => {
    const { events, promptId, turn } = await startTurn("cursor-live");
    notify("cursor/task", {
      toolCallId: "call_agent",
      description: "Review UI events",
      agentId: "child_1",
    });
    notify("session/update", {
      update: {
        sessionUpdate: "tool_call",
        toolCallId: "call_agent",
        title: "Task: Subagent task",
        rawInput: { _toolName: "task" },
        status: "in_progress",
      },
    });
    expect(agentEvents(events).at(-1)).toMatchObject({
      title: "Review UI events",
    });
    reply(promptId, { stopReason: "end_turn" });
    await turn;
  });

  it.each(["cancel", "stop", "exit"])(
    "ignores an in-flight child read after %s",
    async (action) => {
      let resolve!: (runs: unknown[]) => void;
      stores.subagents.mockImplementation(
        () =>
          new Promise((done) => {
            resolve = done;
          }),
      );
      const { events, promptId, turn } = await startTurn("cursor-live");
      emitAgentStart();
      await waitFor(() => !!resolve, "child read");
      if (action === "cancel") await cancelCursorTurn("cursor-live");
      else if (action === "stop") await stopCursorSession("cursor-live");
      else onExit!(1);
      const count = events.length;
      resolve([
        {
          agentId: "child_1",
          toolCallId: "call_agent",
          revision: "1",
          steps: [{ id: "late", kind: "message", text: "Late result" }],
        },
      ]);
      // A stopped ACP client rejects its prompt; cancellation still receives a reply.
      if (action === "cancel") reply(promptId, { stopReason: "cancelled" });
      await turn.catch(() => undefined);
      await new Promise((done) => setTimeout(done, 5));
      expect(
        events
          .slice(count)
          .some(
            (event) =>
              event.type === "agent.step" || event.type === "message.completed",
          ),
      ).toBe(false);
    },
  );

  it("routes attributed child work to its row and leaves parent narration separate", async () => {
    const { events, promptId, turn } = await startTurn("cursor-live");
    emitAgentStart();
    const meta = { parentToolCallId: "call_agent" };
    notify("session/update", {
      sessionId: "cursor_1",
      update: {
        sessionUpdate: "agent_message_chunk",
        _meta: meta,
        content: { type: "text", text: "Child narration" },
      },
    });
    notify("session/update", {
      sessionId: "cursor_1",
      update: {
        sessionUpdate: "tool_call",
        _meta: meta,
        toolCallId: "child_read",
        title: "Read auth.ts",
        kind: "read",
        status: "in_progress",
      },
    });
    notify("session/update", {
      sessionId: "cursor_1",
      update: {
        sessionUpdate: "tool_call_update",
        toolCallId: "child_read",
        status: "completed",
      },
    });
    notify("session/update", {
      sessionId: "cursor_1",
      update: {
        sessionUpdate: "agent_message_chunk",
        content: { type: "text", text: "Parent narration" },
      },
    });
    expect(events.filter((event) => event.type === "message.delta")).toEqual([
      { type: "message.delta", text: "Parent narration" },
    ]);
    expect(events.filter((event) => event.type === "agent.step")).toEqual([
      expect.objectContaining({
        callId: "call_agent",
        kind: "message",
        text: "Child narration",
      }),
      expect.objectContaining({
        callId: "call_agent",
        stepId: "tool:child_read",
        status: "in_progress",
      }),
      expect.objectContaining({
        callId: "call_agent",
        stepId: "tool:child_read",
        status: "completed",
      }),
    ]);
    expect(
      events.some(
        (event) =>
          event.type === "tool.updated" && event.callId === "child_read",
      ),
    ).toBe(false);
    reply(promptId, { stopReason: "end_turn" });
    await turn;
  });

  it("emits request-shaped Cursor todo updates as structured task lists", async () => {
    const { events, promptId, turn } = await startTurn("cursor-live");
    request(71, "cursor/update_todos", {
      toolCallId: "call_todos",
      todos: [
        { content: "Inspect", status: "completed" },
        { content: "Implement", status: "in_progress" },
      ],
    });
    expect(events.at(-1)).toEqual({
      type: "tasks.updated",
      items: [
        { text: "Inspect", status: "completed" },
        { text: "Implement", status: "in_progress" },
      ],
    });
    await waitFor(
      () => parse().some((message) => message.id === 71 && "result" in message),
      "cursor/update_todos response",
    );
    reply(promptId, { stopReason: "end_turn" });
    await turn;
  });

  it("keeps notification-shaped Cursor todo updates compatible", async () => {
    const { events, promptId, turn } = await startTurn("cursor-live");
    notify("_cursor/update_todos", {
      todos: [{ content: "Inspect", status: "pending" }],
    });
    expect(events.at(-1)).toEqual({
      type: "tasks.updated",
      items: [{ text: "Inspect", status: "pending" }],
    });
    reply(promptId, { stopReason: "end_turn" });
    await turn;
  });

  it("preserves Cursor's partial-update signal and task identities", async () => {
    const { events, promptId, turn } = await startTurn("cursor-live");
    request(72, "cursor/update_todos", {
      toolCallId: "call_todos",
      merge: true,
      todos: [
        { id: "2", content: "Implementing the fix", status: "completed" },
      ],
    });
    expect(events.at(-1)).toEqual({
      type: "tasks.updated",
      merge: true,
      items: [
        {
          id: "2",
          text: "Implementing the fix",
          status: "completed",
        },
      ],
    });
    await waitFor(
      () => parse().some((message) => message.id === 72 && "result" in message),
      "partial cursor/update_todos response",
    );
    reply(promptId, { stopReason: "end_turn" });
    await turn;
  });

  it("marks Cursor's redundant todo tool call as internal task activity", async () => {
    const { events, promptId, turn } = await startTurn("cursor-live");
    notify("session/update", {
      sessionId: "cursor_1",
      update: {
        sessionUpdate: "tool_call",
        toolCallId: "call_todos",
        title: "Update TODOs",
        kind: "other",
        status: "pending",
        rawInput: { _toolName: "updateTodos", todos: [] },
      },
    });
    expect(events.at(-1)).toMatchObject({
      type: "tool.updated",
      callId: "call_todos",
      kind: "tasks",
    });
    notify("session/update", {
      sessionId: "cursor_1",
      update: {
        sessionUpdate: "tool_call_update",
        toolCallId: "call_todos",
        kind: "other",
        status: "completed",
      },
    });
    expect(events.at(-1)).toMatchObject({
      type: "tool.updated",
      callId: "call_todos",
      kind: "tasks",
      status: "completed",
    });
    reply(promptId, { stopReason: "end_turn" });
    await turn;
  });

  it("keeps an ACP background task active until the prompt completes", async () => {
    const { events, promptId, turn } = await startTurn("cursor-live");
    let settled = false;
    void turn.then(() => {
      settled = true;
    });

    emitAgentStart();
    notify("session/update", {
      sessionId: "cursor_1",
      update: {
        sessionUpdate: "tool_call_update",
        toolCallId: "call_agent",
        status: "completed",
        rawOutput: { isBackground: true },
      },
    });

    expect(agentEvents(events).at(-1)).toMatchObject({
      type: "tool.updated",
      title: "Explore auth",
      kind: "agent",
      status: "in_progress",
    });
    expect(settled).toBe(false);
    expect(events.some((event) => event.type === "message.completed")).toBe(
      false,
    );

    reply(promptId, { stopReason: "end_turn" });
    await turn;

    expect(agentEvents(events).at(-1)).toMatchObject({
      type: "tool.updated",
      kind: "agent",
      status: "completed",
    });
    expect(settled).toBe(true);
    expect(events.some((event) => event.type === "message.completed")).toBe(
      true,
    );
  });

  it("uses Cursor's task request when raw ACP output omits the background flag", async () => {
    const { events, promptId, turn } = await startTurn("cursor-live");
    emitAgentStart();
    notify("session/update", {
      sessionId: "cursor_1",
      update: {
        sessionUpdate: "tool_call_update",
        toolCallId: "call_agent",
        status: "completed",
      },
    });
    request(99, "cursor/task", {
      toolCallId: "call_agent",
      description: "Explore auth",
      subagentType: "explore",
      agentId: "child_1",
    });

    expect(agentEvents(events).at(-1)).toMatchObject({
      type: "tool.updated",
      title: "Explore auth",
      kind: "agent",
      status: "in_progress",
      detail: "explore subagent",
    });
    await waitFor(
      () => parse().some((message) => message.id === 99 && "result" in message),
      "cursor/task response",
    );

    reply(promptId, { stopReason: "end_turn" });
    await turn;
    expect(agentEvents(events).at(-1)).toMatchObject({ status: "completed" });
  });

  it("does not reopen a foreground task after Cursor reports its duration", async () => {
    const { events, promptId, turn } = await startTurn("cursor-live");
    emitAgentStart();
    notify("session/update", {
      sessionId: "cursor_1",
      update: {
        sessionUpdate: "tool_call_update",
        toolCallId: "call_agent",
        status: "completed",
        rawOutput: { isBackground: false, durationMs: 25 },
      },
    });
    request(100, "cursor/task", {
      toolCallId: "call_agent",
      description: "Explore auth",
      subagentType: "explore",
      agentId: "child_1",
      durationMs: 25,
    });

    expect(agentEvents(events).at(-1)).toMatchObject({
      type: "tool.updated",
      kind: "agent",
      status: "completed",
    });

    reply(promptId, { stopReason: "end_turn" });
    await turn;
    expect(agentEvents(events).at(-1)).toMatchObject({ status: "completed" });
  });
});
