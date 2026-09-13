import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { newSession } from "../session";
import { applyHarnessEvent } from "./apply";

let onStdout: ((line: string) => void) | undefined;
let onSseEvent: ((event: Record<string, unknown>) => void) | undefined;
let onSseEnd: ((error?: string) => void) | undefined;
const spawnChild = vi.fn(async () => {
  onStdout?.("opencode server listening on http://127.0.0.1:4096");
});
const killChild = vi.fn(async () => undefined);
const harnessHttp = vi.fn(
  async (input: {
    url: string;
    method: string;
    body?: string;
  }): Promise<{ status: number; body: string }> => {
    const url = new URL(input.url);
    if (input.method === "POST" && url.pathname === "/session") {
      return { status: 200, body: JSON.stringify({ id: "session_1" }) };
    }
    if (input.method === "GET" && url.pathname === "/session/session_1") {
      return {
        status: 200,
        body: JSON.stringify({ id: "session_1", directory: "/repo" }),
      };
    }
    return { status: 204, body: "" };
  },
);

vi.mock("./child", () => ({
  closeHarnessSse: async () => undefined,
  execChild: async () => "opencode 1.14.19",
  freeHarnessPort: async () => 4096,
  harnessHttp,
  killChild,
  openHarnessSse: async () => undefined,
  resolveOpenCodeBinary: async () => ({ path: "/fake/opencode" }),
  spawnChild,
  unwatchChild: () => undefined,
  watchChild: (_id: string, stdout: (line: string) => void) => {
    onStdout = stdout;
  },
  watchSse: (
    _id: string,
    event: (data: string) => void,
    end?: (error?: string) => void,
  ) => {
    onSseEvent = (value) => event(JSON.stringify(value));
    onSseEnd = end;
  },
}));

const {
  __openCodeTestReset,
  cancelOpenCodeTurn,
  respondOpenCodeApproval,
  respondOpenCodeQuestion,
  sendOpenCodeTurn,
  stopOpenCodeSession,
} = await import("./opencode");
import type { HarnessEvent } from "./types";

const waitFor = async (predicate: () => boolean, label: string) => {
  for (let index = 0; index < 200; index += 1) {
    if (predicate()) return;
    await new Promise((resolve) => setTimeout(resolve, 5));
  }
  throw new Error(`timed out waiting for ${label}`);
};

function turn(events: HarnessEvent[]) {
  return sendOpenCodeTurn({
    sessionId: "opencode-live",
    cwd: "/repo",
    model: "opencode:openrouter/anthropic/claude-sonnet-4.6",
    runtimeMode: "supervised",
    text: "delegate the investigation",
    attachments: [],
    onEvent: (event) => events.push(event),
  });
}

async function startTurn(events: HarnessEvent[]) {
  const done = turn(events);
  await waitFor(
    () =>
      harnessHttp.mock.calls.some(([input]) =>
        input.url.includes("/prompt_async"),
      ),
    "prompt",
  );
  return { done };
}

function sessionCreated(id: string, parentID?: string) {
  onSseEvent?.({
    type: "session.created",
    properties: { sessionID: id, info: { id, parentID, directory: "/repo" } },
  });
}

function askPermission(sessionID: string, id = "permission_child") {
  onSseEvent?.({
    type: "permission.asked",
    properties: {
      id,
      sessionID,
      permission: "external_directory",
      patterns: ["/home/user/*"],
      metadata: { filepath: "/home/user/.gitconfig" },
      tool: { messageID: "message_child", callID: `call_${id}` },
    },
  });
}

function idle(sessionID = "session_1") {
  onSseEvent?.({
    type: "session.status",
    properties: { sessionID, status: { type: "idle" } },
  });
}

beforeEach(() => {
  onStdout = undefined;
  onSseEvent = undefined;
  onSseEnd = undefined;
  spawnChild.mockClear();
  killChild.mockClear();
  harnessHttp.mockClear();
  __openCodeTestReset();
});

afterEach(async () => {
  await stopOpenCodeSession("opencode-live");
  __openCodeTestReset();
});

describe("OpenCode subagent trails", () => {
  const part = (sessionID: string, value: Record<string, unknown>) => onSseEvent?.({
    type: "message.part.updated", properties: { part: { sessionID, ...value } },
  });
  const message = (sessionID: string, id: string, role = "assistant", agent?: string) => onSseEvent?.({
    type: "message.updated", properties: { info: { sessionID, id, role, agent } },
  });
  const task = (callID: string, child: string) => part("session_1", {
    id: `part_${callID}`, type: "tool", tool: "task", callID,
    state: { status: "running", title: `Task ${callID}`, metadata: { sessionId: child } },
  });

  it("pairs concurrent children by metadata and replays their latest parts after creating the row", async () => {
    const events: HarnessEvent[] = [];
    const { done } = await startTurn(events);
    sessionCreated("child_b", "session_1");
    sessionCreated("child_a", "session_1");
    message("child_a", "msg_a");
    message("child_b", "msg_b");
    part("child_b", { id: "prose_b", messageID: "msg_b", type: "text", text: "Second child" });
    for (let i = 0; i < 70; i++) {
      part("child_a", { id: "prose_a", messageID: "msg_a", type: "text", text: `First child ${i}` });
    }
    task("a", "child_a");
    task("b", "child_b");
    idle("child_a");
    expect(events.some((event) => event.type === "message.completed")).toBe(false);
    idle();
    await done;
    const session = events.reduce(applyHarnessEvent, newSession("opencode", "/repo"));
    expect(session.blocks.find((block) => block.tool?.callId === "a")?.agentRun?.steps).toEqual([
      expect.objectContaining({ text: "First child 69" }),
    ]);
    expect(session.blocks.find((block) => block.tool?.callId === "b")?.agentRun?.steps).toEqual([
      expect.objectContaining({ text: "Second child" }),
    ]);
    expect(events.filter((event) => event.type === "message.delta")).toEqual([]);
  });

  it("streams child reasoning and tools, including nested tasks, without user or hidden text", async () => {
    const events: HarnessEvent[] = [];
    const { done } = await startTurn(events);
    task("a", "child");
    sessionCreated("child", "session_1");
    message("child", "user_msg", "user");
    message("child", "hidden_msg", "assistant", "compaction");
    message("child", "assistant_msg");
    part("child", { id: "input", type: "text", messageID: "user_msg", text: "Private prompt" });
    part("child", { id: "hidden", type: "text", messageID: "hidden_msg", text: "Hidden summary" });
    part("child", { id: "think", type: "reasoning", messageID: "assistant_msg", text: "Trace " });
    onSseEvent?.({ type: "message.part.delta", properties: { sessionID: "child", partID: "think", field: "text", delta: "imports" } });
    part("child", { id: "read", type: "tool", tool: "read", callID: "read", messageID: "assistant_msg",
      state: { status: "running", input: { filePath: "auth.ts" } } });
    part("child", { id: "read", type: "tool", tool: "read", callID: "read", messageID: "assistant_msg",
      state: { status: "error", input: { filePath: "auth.ts" }, error: "File missing" } });
    sessionCreated("grandchild", "child");
    message("grandchild", "nested_msg");
    part("grandchild", { id: "nested_text", type: "text", messageID: "nested_msg", text: "Nested answer" });
    part("child", { id: "nested_call", type: "tool", tool: "task", callID: "nested_call", messageID: "assistant_msg",
      state: { status: "running", metadata: { sessionId: "grandchild" } } });
    // Another session on the same server is not part of this run.
    sessionCreated("unrelated");
    message("unrelated", "other_msg");
    part("unrelated", { id: "other", type: "text", messageID: "other_msg", text: "Other session" });
    idle();
    await done;
    const session = events.reduce(applyHarnessEvent, newSession("opencode", "/repo"));
    const steps = session.blocks.find((block) => block.tool?.callId === "a")?.agentRun?.steps;
    expect(steps?.map((step) => step.text)).toEqual(["Trace imports", "Read auth.ts", "Subagent", "Nested answer"]);
    expect(steps?.find((step) => step.toolKind === "read")?.status).toBe("failed");
    expect(session.blocks.filter((block) => block.role === "assistant")).toEqual([]);
  });
});

describe("OpenCode event stream recovery", () => {
  it("fails a cleanly-ended stream and reconnects on the next turn", async () => {
    const firstEvents: HarnessEvent[] = [];
    const first = turn(firstEvents);
    await waitFor(
      () =>
        harnessHttp.mock.calls.some(([input]) =>
          String(input.url).includes("/prompt_async"),
        ),
      "first prompt",
    );

    onSseEnd?.();
    await expect(first).rejects.toThrow(
      "OpenCode event stream ended unexpectedly.",
    );
    expect(firstEvents).toContainEqual({
      type: "session.error",
      message: "OpenCode event stream ended unexpectedly.",
    });

    const secondEvents: HarnessEvent[] = [];
    const second = turn(secondEvents);
    await waitFor(() => spawnChild.mock.calls.length === 2, "fresh transport");
    await waitFor(
      () =>
        harnessHttp.mock.calls.filter(([input]) =>
          String(input.url).includes("/prompt_async"),
        ).length === 2,
      "second prompt",
    );
    onSseEvent?.({
      type: "session.status",
      properties: { sessionID: "session_1", status: { type: "idle" } },
    });
    await second;
    expect(secondEvents).toContainEqual({ type: "message.completed" });
  });
});

describe("OpenCode child permission routing", () => {
  it("queues simultaneous child questions so each stays reachable", async () => {
    const events: HarnessEvent[] = [];
    const { done } = await startTurn(events);
    for (const id of ["child_a", "child_b"]) {
      sessionCreated(id, "session_1");
      onSseEvent?.({
        type: "question.asked",
        properties: {
          id: `question_${id}`,
          sessionID: id,
          questions: [
            {
              question: `Question from ${id}`,
              options: [{ label: "Proceed" }],
            },
          ],
        },
      });
    }
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(
      events.filter((event) => event.type === "question.asked"),
    ).toHaveLength(1);
    for (const id of ["child_a", "child_b"]) {
      const session = events.reduce(
        applyHarnessEvent,
        newSession("opencode", "/repo"),
      );
      const request = session.pendingQuestion!;
      expect(request.questions[0].prompt).toBe(`Question from ${id}`);
      respondOpenCodeQuestion("opencode-live", request.requestId, {
        kind: "skipped",
      });
      await waitFor(
        () =>
          harnessHttp.mock.calls.some(([input]) =>
            input.url.includes(`/question/question_${id}/reject`),
          ),
        "question response",
      );
    }
    expect(
      events.reduce(applyHarnessEvent, newSession("opencode", "/repo"))
        .pendingQuestion,
    ).toBeUndefined();
    idle();
    await done;
  });

  it("ends the turn visibly when a child approval reply fails", async () => {
    const events: HarnessEvent[] = [];
    const { done } = await startTurn(events);
    sessionCreated("session_child", "session_1");
    askPermission("session_child");
    await waitFor(
      () => events.some((event) => event.type === "approval.requested"),
      "child approval",
    );
    const approval = events.find(
      (event) => event.type === "approval.requested",
    )!;
    harnessHttp.mockResolvedValueOnce({
      status: 500,
      body: "Permission reply failed",
    });
    respondOpenCodeApproval("opencode-live", approval.requestId, "allow");
    await waitFor(
      () => events.some((event) => event.type === "session.error"),
      "permission failure",
    );
    await done;
    expect(events).toContainEqual({
      type: "session.error",
      message: "Could not route OpenCode event: Permission reply failed",
    });
  });

  it.each([
    ["session_1", "allow", "once"],
    ["session_1", "deny", "reject"],
    ["session_child", "allow", "once"],
    ["session_child", "deny", "reject"],
    ["session_grandchild", "allow", "once"],
    ["session_grandchild", "deny", "reject"],
  ] as const)(
    "routes %s permission with %s",
    async (sessionID, decision, reply) => {
      const events: HarnessEvent[] = [];
      const { done } = await startTurn(events);
      sessionCreated("session_child", "session_1");
      sessionCreated("session_grandchild", "session_child");
      askPermission(sessionID);

      await waitFor(
        () => events.some((event) => event.type === "approval.requested"),
        "approval",
      );
      const approval = events.find(
        (event) => event.type === "approval.requested",
      )!;
      expect(approval).toMatchObject({
        kind: "external_directory",
        callId: "call_permission_child",
        title: expect.stringContaining("/home/user"),
      });
      const session = events.reduce(
        applyHarnessEvent,
        newSession("opencode", "/repo"),
      );
      expect(
        session.blocks.find(
          (block) => block.approval?.requestId === approval.requestId,
        ),
      ).toMatchObject({
        tool: { callId: "call_permission_child", kind: "external_directory" },
        approval: { requestId: approval.requestId },
      });
      expect(events).not.toContainEqual({ type: "message.completed" });
      respondOpenCodeApproval("opencode-live", approval.requestId, decision);
      await waitFor(
        () =>
          harnessHttp.mock.calls.some(
            ([input]) =>
              new URL(input.url).pathname ===
              "/permission/permission_child/reply",
          ),
        "permission reply",
      );
      expect(harnessHttp).toHaveBeenCalledWith(
        expect.objectContaining({
          method: "POST",
          url: "http://127.0.0.1:4096/permission/permission_child/reply?directory=%2Frepo",
          body: JSON.stringify({ reply }),
        }),
      );
      expect(events).toContainEqual({
        type: "approval.resolved",
        requestId: approval.requestId,
        decision,
      });
      const resolved = events.reduce(
        applyHarnessEvent,
        newSession("opencode", "/repo"),
      );
      expect(
        resolved.blocks.find(
          (block) => block.approval?.requestId === approval.requestId,
        )?.approval?.decided,
      ).toBe(decision);

      idle("session_child");
      expect(events).not.toContainEqual({ type: "message.completed" });
      idle();
      await done;
      expect(events).toContainEqual({ type: "message.completed" });
      expect(events.some((event) => event.type === "session.error")).toBe(
        false,
      );
    },
  );

  it("looks up ancestry for an existing child whose creation was not observed", async () => {
    const events: HarnessEvent[] = [];
    const { done } = await startTurn(events);
    harnessHttp
      .mockResolvedValueOnce({
        status: 200,
        body: JSON.stringify({
          id: "session_grandchild",
          parentID: "session_child",
        }),
      })
      .mockResolvedValueOnce({
        status: 200,
        body: JSON.stringify({ id: "session_child", parentID: "session_1" }),
      });
    askPermission("session_grandchild");
    await waitFor(
      () => events.some((event) => event.type === "approval.requested"),
      "existing child approval",
    );
    for (const sessionID of ["session_grandchild", "session_child"]) {
      expect(harnessHttp).toHaveBeenCalledWith(
        expect.objectContaining({
          method: "GET",
          url: `http://127.0.0.1:4096/session/${sessionID}?directory=%2Frepo`,
        }),
      );
    }
    await cancelOpenCodeTurn("opencode-live");
    await done;
    expect(harnessHttp).toHaveBeenCalledWith(
      expect.objectContaining({
        url: "http://127.0.0.1:4096/permission/permission_child/reply?directory=%2Frepo",
        body: JSON.stringify({ reply: "reject" }),
      }),
    );
  });

  it("ignores unrelated sessions and child transcript, status, and error events", async () => {
    const events: HarnessEvent[] = [];
    const { done } = await startTurn(events);
    sessionCreated("session_child", "session_1");
    sessionCreated("session_other");
    sessionCreated("session_other_child", "session_other");
    const before = [...events];
    askPermission("session_other_child");
    for (const sessionID of ["session_child", "session_other"]) {
      onSseEvent?.({
        type: "message.updated",
        properties: {
          info: {
            id: "message_child",
            sessionID,
            role: "assistant",
            tokens: { input: 123 },
          },
        },
      });
      onSseEvent?.({
        type: "message.part.updated",
        properties: {
          part: {
            id: "part_child",
            sessionID,
            type: "text",
            text: "Child-only text",
          },
        },
      });
      onSseEvent?.({
        type: "message.part.updated",
        properties: {
          part: {
            id: "tool_child",
            sessionID,
            type: "tool",
            tool: "read",
            state: { status: "completed" },
          },
        },
      });
      idle(sessionID);
      onSseEvent?.({
        type: "session.error",
        properties: { sessionID, error: { message: "Child failed" } },
      });
    }
    idle();
    await done;
    expect(events).toEqual([
      ...before,
      { type: "message.completed" },
      { type: "reasoning.completed" },
    ]);
    expect(
      harnessHttp.mock.calls.some(([input]) =>
        input.url.includes("/permission/"),
      ),
    ).toBe(false);
  });

  it("keeps concurrent child requests distinct and deduplicates repeated events", async () => {
    const events: HarnessEvent[] = [];
    const { done } = await startTurn(events);
    sessionCreated("session_child", "session_1");
    onSseEvent?.({
      type: "session.updated",
      properties: { info: { id: "session_sibling", parentID: "session_1" } },
    });
    askPermission("session_child", "permission_a");
    askPermission("session_sibling", "permission_b");
    askPermission("session_child", "permission_a");
    await waitFor(
      () =>
        events.filter((event) => event.type === "approval.requested").length >=
        2,
      "two approvals",
    );
    const approvals = events.filter(
      (event) => event.type === "approval.requested",
    );
    expect(approvals).toHaveLength(2);
    expect(approvals[0].requestId).not.toBe(approvals[1].requestId);
    respondOpenCodeApproval("opencode-live", approvals[1].requestId, "deny");
    respondOpenCodeApproval("opencode-live", approvals[0].requestId, "allow");
    idle();
    await done;
    const replies = harnessHttp.mock.calls.filter(([input]) =>
      input.url.includes("/permission/"),
    );
    expect(
      replies.map(([input]) => [
        new URL(input.url).pathname,
        JSON.parse(input.body!),
      ]),
    ).toEqual([
      ["/permission/permission_b/reply", { reply: "reject" }],
      ["/permission/permission_a/reply", { reply: "once" }],
    ]);
  });

  it("surfaces ancestry lookup errors instead of silently losing requests", async () => {
    const events: HarnessEvent[] = [];
    const { done } = await startTurn(events);
    harnessHttp.mockResolvedValueOnce({
      status: 500,
      body: "Session lookup failed",
    });
    askPermission("session_child");
    await done;
    expect(events).toContainEqual({
      type: "session.error",
      message: "Could not route OpenCode event: Session lookup failed",
    });
    expect(events.some((event) => event.type === "approval.requested")).toBe(
      false,
    );
  });

  it("does not show a late child approval after cancellation", async () => {
    const events: HarnessEvent[] = [];
    const { done } = await startTurn(events);
    let resolveLookup!: (response: { status: number; body: string }) => void;
    harnessHttp.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveLookup = resolve;
        }),
    );
    askPermission("session_child");
    await cancelOpenCodeTurn("opencode-live");
    resolveLookup({
      status: 200,
      body: JSON.stringify({ id: "session_child", parentID: "session_1" }),
    });
    await done;
    expect(events.some((event) => event.type === "approval.requested")).toBe(
      false,
    );
  });

  it.each(["answered", "skipped"] as const)(
    "routes child questions when %s",
    async (kind) => {
      const events: HarnessEvent[] = [];
      const { done } = await startTurn(events);
      sessionCreated("session_child", "session_1");
      onSseEvent?.({
        type: "question.asked",
        properties: {
          id: "question_child",
          sessionID: "session_child",
          questions: [
            {
              question: "Which directory?",
              options: [{ label: "Repo", description: "Use the repository" }],
            },
          ],
        },
      });
      await waitFor(
        () => events.some((event) => event.type === "question.asked"),
        "child question",
      );
      const request = events.find((event) => event.type === "question.asked")!;
      const question = request.questions[0];
      respondOpenCodeQuestion(
        "opencode-live",
        request.requestId,
        kind === "answered"
          ? { kind, answers: { [question.id]: [question.options[0].id] } }
          : { kind },
      );
      idle();
      await done;
      expect(harnessHttp).toHaveBeenCalledWith(
        expect.objectContaining({
          method: "POST",
          url: `http://127.0.0.1:4096/question/question_child/${kind === "answered" ? "reply" : "reject"}?directory=%2Frepo`,
          body: JSON.stringify(
            kind === "answered" ? { answers: [["Repo"]] } : {},
          ),
        }),
      );
      expect(events).toContainEqual({
        type: "question.resolved",
        requestId: request.requestId,
        decision: kind,
      });
    },
  );
});
