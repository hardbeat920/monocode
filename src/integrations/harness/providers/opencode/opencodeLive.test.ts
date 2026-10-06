import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  newSession,
  type RuntimeMode,
} from "../../../../features/sessions/model/session";
import { applyHarnessEvent } from "../../core/apply";
import {
  setHarnessModels,
  setProjectHarnessModels,
  resetHarnessModelOverlays,
} from "../../../../features/sessions/model/models";

let onStdout: ((line: string) => void) | undefined;
let onChildExit: ((code: number | null) => void) | undefined;
let onSseEvent: ((event: Record<string, unknown>) => void) | undefined;
let onSseEnd: ((error?: string) => void) | undefined;
let sessionMessages: Array<{
  info?: Record<string, unknown>;
  parts?: unknown[];
}> = [];
let promptMessageID: string | undefined;
let fakeSessionStatus = "idle";
const execChild = vi.fn(async (_path: string, args: string[]) =>
  args[0] === "--version"
    ? "opencode 1.14.19"
    : args[0] === "debug" && args[1] === "paths"
      ? "data       /isolated/data/opencode"
      : ["build", "plan", "general", "explore"]
          .map(
            (name) =>
              `${name} (${["build", "plan"].includes(name) ? "primary" : "subagent"})\n[]`,
          )
          .join("\n"),
);
const spawnChild = vi.fn(async (..._args: unknown[]) => {
  onStdout?.("opencode server listening on http://127.0.0.1:4096");
});
const killChild = vi.fn(async () => undefined);
const closeHarnessSse = vi.fn(async (_id: string) => undefined);
const defaultHarnessHttp = async (input: {
  url: string;
  method: string;
  body?: string;
}): Promise<{ status: number; body: string }> => {
  const url = new URL(input.url);
  if (url.pathname === "/agent" || url.pathname === "/config") {
    const env = spawnChild.mock.calls[
      spawnChild.mock.calls.length - 1
    ]?.[6] as Record<string, string>;
    const config = JSON.parse(env.OPENCODE_CONFIG_CONTENT);
    const agents = Object.entries(config.agent).map(([name, value]) => ({
      name,
      mode: ["build", "plan"].includes(name) ? "primary" : "subagent",
      permission: Object.entries(
        (value as { permission: Record<string, unknown> }).permission,
      ).flatMap(([permission, action]) =>
        typeof action === "string"
          ? [{ permission, pattern: "*", action }]
          : Object.entries(action as Record<string, string>).map(
              ([pattern, action]) => ({ permission, pattern, action }),
            ),
      ),
    }));
    return {
      status: 200,
      body: JSON.stringify(url.pathname === "/agent" ? agents : config),
    };
  }
  if (url.pathname.endsWith("/prompt_async")) {
    const body = JSON.parse(input.body!);
    promptMessageID = body.messageID;
    fakeSessionStatus = "busy";
    const info = {
      id: promptMessageID,
      sessionID: "session_1",
      role: "user",
      agent: body.agent,
      time: { created: Date.now() },
    };
    sessionMessages.push({ info, parts: body.parts });
    onSseEvent?.({ type: "message.updated", properties: { info } });
  }
  if (url.pathname === "/session/status")
    return {
      status: 200,
      body: JSON.stringify({ session_1: { type: fakeSessionStatus } }),
    };
  if (input.method === "POST" && url.pathname === "/session") {
    return { status: 200, body: JSON.stringify({ id: "session_1" }) };
  }
  if (input.method === "GET" && url.pathname === "/session/session_1") {
    return {
      status: 200,
      body: JSON.stringify({ id: "session_1", directory: "/repo" }),
    };
  }
  if (input.method === "GET" && url.pathname === "/session/session_1/message") {
    return { status: 200, body: JSON.stringify(sessionMessages) };
  }
  return { status: 204, body: "" };
};
const harnessHttp = vi.fn(defaultHarnessHttp);

vi.mock("../../core/child", () => ({
  closeHarnessSse,
  execChild,
  freeHarnessPort: async () => 4096,
  harnessHttp,
  killChild,
  openHarnessSse: async () => undefined,
  resolveOpenCodeBinary: async () => ({ path: "/fake/opencode" }),
  spawnChild,
  unwatchChild: () => undefined,
  watchChild: (
    _id: string,
    stdout: (line: string) => void,
    exit: (code: number | null) => void,
  ) => {
    onStdout = stdout;
    onChildExit = exit;
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
  bindOpenCodeSession,
  cancelOpenCodeTurn,
  compactOpenCodeContext,
  steerOpenCodeTurn,
  respondOpenCodeApproval,
  respondOpenCodeQuestion,
  rewindOpenCodeLastTurn,
  sendOpenCodeTurn,
  stopOpenCodeSession,
} = await import("./opencode");
import { TurnNotReadyError, type HarnessEvent } from "../../core/types";

const waitFor = async (predicate: () => boolean, label: string) => {
  for (let index = 0; index < 200; index += 1) {
    if (predicate()) return;
    await new Promise((resolve) => setTimeout(resolve, 5));
  }
  throw new Error(`timed out waiting for ${label}`);
};

function turn(
  events: HarnessEvent[],
  options: { runtimeMode?: RuntimeMode; onAccepted?: () => void } = {},
) {
  return sendOpenCodeTurn({
    sessionId: "opencode-live",
    cwd: "/repo",
    model: "opencode:openrouter/anthropic/claude-sonnet-4.6",
    runtimeMode: options.runtimeMode ?? "supervised",
    text: "delegate the investigation",
    attachments: [],
    onAccepted: options.onAccepted,
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
  if (sessionID === "session_1") {
    fakeSessionStatus = "idle";
    if (promptMessageID)
      sessionMessages.push({
        info: {
          id: `assistant_${sessionMessages.length}`,
          role: "assistant",
          agent: "build",
          parentID: promptMessageID,
          finish: "stop",
          time: { completed: Date.now() },
        },
        parts: [],
      });
  }
  onSseEvent?.({
    type: "session.status",
    properties: { sessionID, status: { type: "idle" } },
  });
}

beforeEach(() => {
  onStdout = undefined;
  onChildExit = undefined;
  onSseEvent = undefined;
  onSseEnd = undefined;
  sessionMessages = [];
  promptMessageID = undefined;
  fakeSessionStatus = "idle";
  execChild.mockClear();
  spawnChild.mockClear();
  killChild.mockClear();
  closeHarnessSse.mockReset();
  closeHarnessSse.mockResolvedValue(undefined);
  harnessHttp.mockReset();
  harnessHttp.mockImplementation(defaultHarnessHttp);
  __openCodeTestReset();
});

afterEach(async () => {
  vi.useRealTimers();
  await stopOpenCodeSession("opencode-live");
  __openCodeTestReset();
  resetHarnessModelOverlays();
});

it("reports when OpenCode accepts a turn", async () => {
  const events: HarnessEvent[] = [];
  const onAccepted = vi.fn();
  const done = turn(events, { onAccepted });

  await waitFor(() => onAccepted.mock.calls.length === 1, "turn acceptance");
  idle();
  await done;
  expect(onAccepted).toHaveBeenCalledOnce();
});

describe("OpenCode subagent trails", () => {
  const part = (sessionID: string, value: Record<string, unknown>) =>
    onSseEvent?.({
      type: "message.part.updated",
      properties: { part: { sessionID, ...value } },
    });
  const message = (
    sessionID: string,
    id: string,
    role = "assistant",
    agent?: string,
    modelID?: string,
  ) =>
    onSseEvent?.({
      type: "message.updated",
      properties: { info: { sessionID, id, role, agent, modelID } },
    });
  const task = (callID: string, child: string) =>
    part("session_1", {
      id: `part_${callID}`,
      type: "tool",
      tool: "task",
      callID,
      state: {
        status: "running",
        title: `Task ${callID}`,
        metadata: { sessionId: child },
      },
    });

  it("preserves completed child text when a late delta arrives", async () => {
    const events: HarnessEvent[] = [];
    const { done } = await startTurn(events);
    task("a", "child");
    sessionCreated("child", "session_1");
    message("child", "child_message");
    part("child", {
      id: "child_text",
      messageID: "child_message",
      type: "text",
      text: "Final answer",
      time: { start: 1, end: 2 },
    });
    onSseEvent?.({
      type: "message.part.delta",
      properties: {
        sessionID: "child",
        partID: "child_text",
        field: "text",
        delta: " stale suffix",
      },
    });
    idle();
    await done;
    const session = events.reduce(
      applyHarnessEvent,
      newSession("opencode", "/repo"),
    );
    expect(
      session.blocks.find((block) => block.tool?.callId === "a")?.agentRun
        ?.steps,
    ).toEqual([expect.objectContaining({ text: "Final answer" })]);
  });

  it("pairs concurrent children by metadata and replays their latest parts after creating the row", async () => {
    const events: HarnessEvent[] = [];
    const { done } = await startTurn(events);
    sessionCreated("child_b", "session_1");
    sessionCreated("child_a", "session_1");
    message("child_a", "msg_a", "assistant", undefined, "claude-haiku-4-5");
    message("child_b", "msg_b");
    part("child_b", {
      id: "prose_b",
      messageID: "msg_b",
      type: "text",
      text: "Second child",
    });
    for (let i = 0; i < 70; i++) {
      part("child_a", {
        id: "prose_a",
        messageID: "msg_a",
        type: "text",
        text: `First child ${i}`,
      });
    }
    task("a", "child_a");
    task("b", "child_b");
    idle("child_a");
    expect(events.some((event) => event.type === "message.completed")).toBe(
      false,
    );
    idle();
    await done;
    const session = events.reduce(
      applyHarnessEvent,
      newSession("opencode", "/repo"),
    );
    expect(
      session.blocks.find((block) => block.tool?.callId === "a")?.agentRun
        ?.model,
    ).toBe("claude-haiku-4-5");
    expect(
      session.blocks.find((block) => block.tool?.callId === "a")?.agentRun
        ?.steps,
    ).toEqual([expect.objectContaining({ text: "First child 69" })]);
    expect(
      session.blocks.find((block) => block.tool?.callId === "b")?.agentRun
        ?.steps,
    ).toEqual([expect.objectContaining({ text: "Second child" })]);
    expect(events.filter((event) => event.type === "message.delta")).toEqual(
      [],
    );
  });

  it("streams child reasoning and tools, including nested tasks, without user or hidden text", async () => {
    const events: HarnessEvent[] = [];
    const { done } = await startTurn(events);
    task("a", "child");
    sessionCreated("child", "session_1");
    message("child", "user_msg", "user");
    message("child", "hidden_msg", "assistant", "compaction");
    message("child", "assistant_msg");
    part("child", {
      id: "input",
      type: "text",
      messageID: "user_msg",
      text: "Private prompt",
    });
    part("child", {
      id: "hidden",
      type: "text",
      messageID: "hidden_msg",
      text: "Hidden summary",
    });
    part("child", {
      id: "think",
      type: "reasoning",
      messageID: "assistant_msg",
      text: "Trace ",
    });
    onSseEvent?.({
      type: "message.part.delta",
      properties: {
        sessionID: "child",
        partID: "think",
        field: "text",
        delta: "imports",
      },
    });
    part("child", {
      id: "read",
      type: "tool",
      tool: "read",
      callID: "read",
      messageID: "assistant_msg",
      state: { status: "running", input: { filePath: "auth.ts" } },
    });
    part("child", {
      id: "read",
      type: "tool",
      tool: "read",
      callID: "read",
      messageID: "assistant_msg",
      state: {
        status: "error",
        input: { filePath: "auth.ts" },
        error: "File missing",
      },
    });
    sessionCreated("grandchild", "child");
    message("grandchild", "nested_msg");
    part("grandchild", {
      id: "nested_text",
      type: "text",
      messageID: "nested_msg",
      text: "Nested answer",
    });
    part("child", {
      id: "nested_call",
      type: "tool",
      tool: "task",
      callID: "nested_call",
      messageID: "assistant_msg",
      state: { status: "running", metadata: { sessionId: "grandchild" } },
    });
    // Another session on the same server is not part of this run.
    sessionCreated("unrelated");
    message("unrelated", "other_msg");
    part("unrelated", {
      id: "other",
      type: "text",
      messageID: "other_msg",
      text: "Other session",
    });
    idle();
    await done;
    const session = events.reduce(
      applyHarnessEvent,
      newSession("opencode", "/repo"),
    );
    const steps = session.blocks.find((block) => block.tool?.callId === "a")
      ?.agentRun?.steps;
    expect(steps?.map((step) => step.text)).toEqual([
      "Trace imports",
      "Read auth.ts",
      "Subagent",
      "Nested answer",
    ]);
    expect(steps?.find((step) => step.toolKind === "read")?.status).toBe(
      "failed",
    );
    expect(steps?.find((step) => step.toolKind === "read")?.detail).toBe(
      "File missing",
    );
    expect(
      session.blocks.filter((block) => block.role === "assistant"),
    ).toEqual([]);
  });
});

describe("OpenCode event stream recovery", () => {
  it("reverts a rejected file turn before continuing a resumed session", async () => {
    sessionMessages = [
      {
        info: {
          id: "message_bad_file",
          sessionID: "session_1",
          role: "user",
          time: { created: 1 },
        },
        parts: [
          {
            type: "file",
            mime: "application/octet-stream",
            filename: "Info.plist",
          },
        ],
      },
      {
        info: {
          id: "message_bad_reply",
          parentID: "message_bad_file",
          sessionID: "session_1",
          role: "assistant",
          time: { created: 2 },
          error: {
            data: {
              message:
                "'file part media type application/octet-stream' functionality not supported.",
            },
          },
        },
        parts: [],
      },
    ];
    bindOpenCodeSession("opencode-live", "session_1", "/repo");

    const events: HarnessEvent[] = [];
    const done = turn(events);
    await waitFor(
      () =>
        harnessHttp.mock.calls.some(([input]) =>
          input.url.includes("/session/session_1/revert"),
        ),
      "attachment turn recovery",
    );
    await waitFor(
      () =>
        harnessHttp.mock.calls.some(([input]) =>
          input.url.includes("/prompt_async"),
        ),
      "resumed prompt",
    );

    const revertIndex = harnessHttp.mock.calls.findIndex(([input]) =>
      input.url.includes("/session/session_1/revert"),
    );
    const promptIndex = harnessHttp.mock.calls.findIndex(([input]) =>
      input.url.includes("/prompt_async"),
    );
    expect(revertIndex).toBeGreaterThanOrEqual(0);
    expect(promptIndex).toBeGreaterThan(revertIndex);
    expect(harnessHttp.mock.calls[revertIndex]?.[0]).toMatchObject({
      method: "POST",
      body: JSON.stringify({ messageID: "message_bad_file" }),
    });

    idle();
    await done;
    expect(events).toContainEqual({ type: "message.completed" });
  });

  it("preserves history when a later assistant turn succeeded", async () => {
    sessionMessages = [
      {
        info: {
          id: "message_bad_file",
          role: "user",
          time: { created: 1 },
        },
        parts: [{ type: "file", mime: "application/octet-stream" }],
      },
      {
        info: {
          id: "message_bad_reply",
          parentID: "message_bad_file",
          role: "assistant",
          time: { created: 2 },
          error: {
            data: {
              message:
                "'file part media type application/octet-stream' functionality not supported.",
            },
          },
        },
        parts: [],
      },
      {
        info: {
          id: "message_recovered_reply",
          role: "assistant",
          time: { created: 3 },
        },
        parts: [{ type: "text", text: "Recovered" }],
      },
    ];
    bindOpenCodeSession("opencode-live", "session_1", "/repo");

    const events: HarnessEvent[] = [];
    const done = turn(events);
    await waitFor(
      () =>
        harnessHttp.mock.calls.some(([input]) =>
          input.url.includes("/prompt_async"),
        ),
      "resumed prompt",
    );
    expect(
      harnessHttp.mock.calls.some(([input]) => input.url.includes("/revert")),
    ).toBe(false);

    idle();
    await done;
  });

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
    idle();
    await second;
    expect(secondEvents).toContainEqual({ type: "message.completed" });
  });
});

describe("OpenCode edit recovery", () => {
  it("reverts the latest user message before resending an edited prompt", async () => {
    sessionMessages = [
      {
        info: {
          id: "message_first",
          role: "user",
          time: { created: 1 },
        },
      },
      {
        info: {
          id: "message_first_reply",
          role: "assistant",
          time: { created: 2 },
        },
      },
      {
        info: {
          id: "message_latest",
          role: "user",
          time: { created: 3 },
        },
      },
    ];
    bindOpenCodeSession("opencode-live", "session_1", "/repo");

    const rewind = rewindOpenCodeLastTurn({
      sessionId: "opencode-live",
      cwd: "/repo",
      model: "opencode:openrouter/anthropic/claude-sonnet-4.6",
      runtimeMode: "supervised",
      onEvent: () => undefined,
    });
    await waitFor(
      () =>
        harnessHttp.mock.calls.some(([input]) =>
          input.url.includes("/session/session_1/revert"),
        ),
      "edited message revert",
    );

    const request = harnessHttp.mock.calls.find(([input]) =>
      input.url.includes("/session/session_1/revert"),
    )?.[0];
    expect(request).toMatchObject({
      method: "POST",
      body: JSON.stringify({ messageID: "message_latest" }),
    });
    expect(await rewind).toEqual({ submitted: false });
  });

  it("uses response order when a user timestamp is missing", async () => {
    sessionMessages = [
      {
        info: {
          id: "message_older",
          role: "user",
          time: { created: 100 },
        },
      },
      {
        info: {
          id: "message_latest",
          role: "user",
        },
      },
    ];
    bindOpenCodeSession("opencode-live", "session_1", "/repo");

    const rewind = rewindOpenCodeLastTurn({
      sessionId: "opencode-live",
      cwd: "/repo",
      model: "opencode:openrouter/anthropic/claude-sonnet-4.6",
      runtimeMode: "supervised",
      onEvent: () => undefined,
    });
    await waitFor(
      () =>
        harnessHttp.mock.calls.some(([input]) =>
          input.url.includes("/session/session_1/revert"),
        ),
      "edited message revert",
    );

    const request = harnessHttp.mock.calls.find(([input]) =>
      input.url.includes("/session/session_1/revert"),
    )?.[0];
    expect(request).toMatchObject({
      method: "POST",
      body: JSON.stringify({ messageID: "message_latest" }),
    });
    await expect(rewind).resolves.toEqual({ submitted: false });
  });
});
describe("OpenCode access modes", () => {
  it("updates a live session when access changes and auto-allows residual full-access prompts", async () => {
    const events: HarnessEvent[] = [];
    const first = turn(events);
    await waitFor(
      () =>
        harnessHttp.mock.calls.some(([input]) =>
          input.url.includes("/prompt_async"),
        ),
      "first prompt",
    );
    idle();
    await first;

    const second = turn(events, { runtimeMode: "full-access" });
    await waitFor(
      () =>
        harnessHttp.mock.calls.some(
          ([input]) =>
            input.method === "PATCH" &&
            new URL(input.url).pathname === "/session/session_1",
        ),
      "permission update",
    );
    expect(harnessHttp).toHaveBeenCalledWith(
      expect.objectContaining({
        method: "PATCH",
        url: "http://127.0.0.1:4096/session/session_1?directory=%2Frepo",
        body: JSON.stringify({
          permission: [{ permission: "*", pattern: "*", action: "allow" }],
        }),
      }),
    );
    await waitFor(
      () =>
        harnessHttp.mock.calls.filter(([input]) =>
          input.url.includes("/prompt_async"),
        ).length === 2,
      "second prompt",
    );

    askPermission("session_1", "permission_residual");
    await waitFor(
      () =>
        harnessHttp.mock.calls.some(
          ([input]) =>
            new URL(input.url).pathname ===
            "/permission/permission_residual/reply",
        ),
      "automatic full-access reply",
    );
    expect(harnessHttp).toHaveBeenCalledWith(
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({ reply: "once" }),
      }),
    );
    expect(events.some((event) => event.type === "approval.requested")).toBe(
      false,
    );

    idle();
    await second;
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

it("closes an event stream that ended on its own when the session stops", async () => {
  const events: HarnessEvent[] = [];
  const { done } = await startTurn(events);
  onSseEnd?.("stream closed");
  await done.catch(() => undefined);
  closeHarnessSse.mockClear();

  await stopOpenCodeSession("opencode-live");
  expect(closeHarnessSse).toHaveBeenCalledWith("opencode-live");
});

it("closes the event stream after the server exits on its own", async () => {
  const events: HarnessEvent[] = [];
  const { done } = await startTurn(events);
  onChildExit?.(1);
  await expect(done).rejects.toThrow("OpenCode server exited");
  expect(events).toContainEqual({ type: "session.ended", code: 1 });

  await stopOpenCodeSession("opencode-live");
  expect(closeHarnessSse).toHaveBeenCalledExactlyOnceWith("opencode-live");
  expect(killChild).toHaveBeenCalledExactlyOnceWith("opencode-live");
});

it("still kills the child when closing an ended stream fails", async () => {
  const events: HarnessEvent[] = [];
  const { done } = await startTurn(events);
  onSseEnd?.("stream closed");
  await expect(done).rejects.toThrow("stream closed");
  closeHarnessSse.mockClear();
  killChild.mockClear();
  closeHarnessSse.mockRejectedValueOnce(new Error("SSE close failed"));

  await expect(stopOpenCodeSession("opencode-live")).resolves.toBeUndefined();
  expect(closeHarnessSse).toHaveBeenCalledExactlyOnceWith("opencode-live");
  expect(killChild).toHaveBeenCalledExactlyOnceWith("opencode-live");
});

describe("OpenCode review regressions", () => {
  const sessionInput = (
    onEvent: (event: HarnessEvent) => void = () => undefined,
  ) => ({
    sessionId: "opencode-live",
    cwd: "/repo",
    model: "opencode:openai/review",
    runtimeMode: "supervised" as const,
    onEvent,
  });
  const emitMessage = (
    value: Record<string, unknown>,
    parts: unknown[] = [],
  ) => {
    const info = { time: { created: Date.now() }, ...value };
    sessionMessages.push({ info, parts });
    onSseEvent?.({
      type: "message.updated",
      properties: { info: { sessionID: "session_1", ...info } },
    });
  };
  const rawIdle = () =>
    onSseEvent?.({
      type: "session.status",
      properties: { sessionID: "session_1", status: { type: "idle" } },
    });

  it("rejects OpenCode 2 before spawning a v1 server", async () => {
    execChild.mockResolvedValueOnce("opencode v2.0.20");
    await expect(turn([])).rejects.toThrow("OpenCode 2 uses a different API");
    expect(spawnChild).not.toHaveBeenCalled();
  });

  it("rejects effective higher-priority permissions before creating a prompt", async () => {
    harnessHttp.mockImplementation(async (input) =>
      new URL(input.url).pathname === "/agent"
        ? {
            status: 200,
            body: JSON.stringify([
              {
                name: "injected",
                permission: [
                  { permission: "*", pattern: "*", action: "ask" },
                  { permission: "bash", pattern: "*", action: "allow" },
                ],
              },
            ]),
          }
        : defaultHarnessHttp(input),
    );
    await expect(turn([])).rejects.toThrow("grants tools beyond");
    expect(
      harnessHttp.mock.calls.some(([input]) =>
        input.url.includes("/prompt_async"),
      ),
    ).toBe(false);
    expect(killChild).toHaveBeenCalledWith("opencode-live");
  });

  it("rejects a higher-priority primary child-tool grant", async () => {
    harnessHttp.mockImplementation(async (input) =>
      new URL(input.url).pathname === "/config"
        ? {
            status: 200,
            body: JSON.stringify({ experimental: { primary_tools: ["bash"] } }),
          }
        : defaultHarnessHttp(input),
    );
    await expect(turn([])).rejects.toThrow("grants tools beyond");
    expect(
      harnessHttp.mock.calls.some(([input]) =>
        input.url.includes("/prompt_async"),
      ),
    ).toBe(false);
  });

  it("rejects an effective subagent Explore grant before creating a prompt", async () => {
    harnessHttp.mockImplementation(async (input) =>
      new URL(input.url).pathname === "/agent"
        ? {
            status: 200,
            body: JSON.stringify([
              {
                name: "explore",
                mode: "subagent",
                permission: [
                  { permission: "*", pattern: "*", action: "deny" },
                  { permission: "task", pattern: "*", action: "deny" },
                  { permission: "task", pattern: "explore", action: "allow" },
                ],
              },
            ]),
          }
        : defaultHarnessHttp(input),
    );
    await expect(
      sendOpenCodeTurn({
        ...sessionInput(),
        intent: "plan",
        text: "Plan safely",
      }),
    ).rejects.toThrow("grants tools beyond");
    expect(
      harnessHttp.mock.calls.some(([input]) =>
        input.url.includes("/prompt_async"),
      ),
    ).toBe(false);
    expect(killChild).toHaveBeenCalledWith("opencode-live");
  });

  it("starts Plan and its subagents with the managed permission policy", async () => {
    const done = sendOpenCodeTurn({
      ...sessionInput(),
      runtimeMode: "full-access",
      intent: "plan",
      text: "Plan this change",
    });
    await waitFor(() => promptMessageID !== undefined, "Plan prompt");
    const env = spawnChild.mock.calls[0]?.[6] as Record<string, string>;
    const config = JSON.parse(env.OPENCODE_CONFIG_CONTENT);
    expect(config.permission["*"]).toBe("deny");
    expect(config.permission.task).toBe("deny");
    expect(config.agent.plan.permission.task).toEqual({
      "*": "deny",
      explore: "allow",
    });
    expect(config.agent.general.permission.task).toBe("deny");
    expect(config.agent.explore.permission.task).toBe("deny");
    expect(config.agent.general.permission["*"]).toBe("deny");
    expect(config.permission.external_directory).toEqual({
      "*": "deny",
      "/isolated/data/opencode/tool-output/*": "allow",
    });
    const session = harnessHttp.mock.calls.find(
      ([input]) => new URL(input.url).pathname === "/session",
    )?.[0];
    expect(JSON.parse(session!.body!).permission).toContainEqual({
      permission: "external_directory",
      pattern: "/isolated/data/opencode/tool-output/*",
      action: "allow",
    });
    expect(config.experimental.primary_tools).toEqual([]);
    idle();
    await done;
  });

  it("rejects an unsafe tool-output directory before starting the server", async () => {
    execChild
      .mockResolvedValueOnce("opencode v1.14.19")
      .mockResolvedValueOnce("build (primary)\n[]")
      .mockResolvedValueOnce("data       /other/*");
    await expect(turn([])).rejects.toThrow("safe data directory");
    expect(spawnChild).not.toHaveBeenCalled();
  });

  const planTaskScopes = [
    { label: "missing", patterns: undefined, reply: "reject" },
    { label: "null", patterns: null, reply: "reject" },
    { label: "empty", patterns: [], reply: "reject" },
    { label: "a scalar", patterns: "explore", reply: "reject" },
    { label: "an object", patterns: { agent: "explore" }, reply: "reject" },
    { label: "a non-string", patterns: [42], reply: "reject" },
    { label: "an empty string", patterns: [""], reply: "reject" },
    { label: "padded explore", patterns: [" explore "], reply: "reject" },
    { label: "mixed types", patterns: ["explore", 42], reply: "reject" },
    { label: "mixed null", patterns: ["explore", null], reply: "reject" },
    { label: "another agent", patterns: ["general"], reply: "reject" },
    {
      label: "mixed agents",
      patterns: ["explore", "general"],
      reply: "reject",
    },
    { label: "explore", patterns: ["explore"], reply: "once" },
    {
      label: "repeated explore",
      patterns: ["explore", "explore"],
      reply: "once",
    },
  ];
  it.each(
    planTaskScopes.flatMap((scope) =>
      ["session_1", "session_child", "session_grandchild"].map((sessionID) => ({
        ...scope,
        sessionID,
        reply: sessionID === "session_1" ? scope.reply : "reject",
      })),
    ),
  )(
    "replies $reply to a Plan task with $label scope in $sessionID",
    async ({ patterns, reply, sessionID }) => {
      const events: HarnessEvent[] = [];
      const done = sendOpenCodeTurn({
        ...sessionInput((event) => events.push(event)),
        intent: "plan",
        runtimeMode: "full-access",
        text: "Explore the change",
      });
      await waitFor(() => promptMessageID !== undefined, "Plan prompt");
      if (sessionID !== "session_1")
        sessionCreated("session_child", "session_1");
      if (sessionID === "session_grandchild")
        sessionCreated(sessionID, "session_child");
      onSseEvent?.({
        type: "permission.asked",
        properties: {
          id: "plan_task_request",
          sessionID,
          permission: "task",
          ...(patterns !== undefined ? { patterns } : {}),
          metadata: { subagent_type: "explore" },
        },
      });
      await waitFor(
        () =>
          harnessHttp.mock.calls.some(([input]) =>
            input.url.includes("/permission/plan_task_request/reply"),
          ),
        "Plan task reply",
      );
      const request = harnessHttp.mock.calls.find(([input]) =>
        input.url.includes("/permission/plan_task_request/reply"),
      )![0];
      expect(request.method).toBe("POST");
      expect(request.url).toBe(
        "http://127.0.0.1:4096/permission/plan_task_request/reply?directory=%2Frepo",
      );
      expect(JSON.parse(request.body!)).toEqual({ reply });
      expect(events.some((event) => event.type === "approval.requested")).toBe(
        false,
      );
      idle();
      await done;
    },
  );

  it("keeps the Plan agent on a steered follow-up", async () => {
    const done = sendOpenCodeTurn({
      ...sessionInput(),
      intent: "plan",
      text: "Plan this change",
    });
    await waitFor(() => promptMessageID !== undefined, "Plan prompt");
    await steerOpenCodeTurn({
      ...sessionInput(),
      modelSettings: { agent: "build" },
      text: "Also account for Windows",
    });
    const requests = harnessHttp.mock.calls.filter(([input]) =>
      input.url.includes("/prompt_async"),
    );
    expect(requests.map(([input]) => JSON.parse(input.body!).agent)).toEqual([
      "plan",
      "plan",
    ]);
    idle();
    await done;
  });

  it("does not submit a resumed prompt after its permission PATCH fails", async () => {
    bindOpenCodeSession("opencode-live", "session_1", "/repo");
    harnessHttp.mockImplementation(async (input) =>
      input.method === "PATCH"
        ? { status: 500, body: "Permission update failed" }
        : defaultHarnessHttp(input),
    );
    await expect(turn([])).rejects.toThrow("Permission update failed");
    expect(
      harnessHttp.mock.calls.some(([input]) =>
        input.url.includes("/prompt_async"),
      ),
    ).toBe(false);
    expect(killChild).toHaveBeenCalled();
  });

  it("reports a failed abort and kills the owned server", async () => {
    const events: HarnessEvent[] = [];
    const { done } = await startTurn(events);
    harnessHttp.mockResolvedValueOnce({ status: 500, body: "Abort rejected" });
    await expect(cancelOpenCodeTurn("opencode-live")).rejects.toThrow(
      "Abort rejected",
    );
    await done;
    expect(killChild).toHaveBeenCalledWith("opencode-live");
    expect(events).toContainEqual({
      type: "session.error",
      message: "Could not confirm OpenCode cancellation: Abort rejected",
    });
  });

  it("kills the server and releases the turn if closing its stream fails", async () => {
    const { done } = await startTurn([]);
    closeHarnessSse.mockRejectedValueOnce(new Error("Stream close rejected"));
    await expect(cancelOpenCodeTurn("opencode-live")).resolves.toBeUndefined();
    await done;
    expect(killChild).toHaveBeenCalledWith("opencode-live");
  });

  it("cancels a resumed session during startup before submitting a prompt", async () => {
    bindOpenCodeSession("opencode-live", "session_1", "/repo");
    let releaseVersion!: (value: string) => void;
    let checkingVersion = false;
    execChild.mockImplementationOnce(() => {
      checkingVersion = true;
      return new Promise<string>((resolve) => {
        releaseVersion = resolve;
      });
    });
    const done = turn([]);
    await waitFor(() => checkingVersion, "version lookup");
    const cancelled = cancelOpenCodeTurn("opencode-live");
    releaseVersion("1.14.19");
    await cancelled;
    await done;
    expect(
      harnessHttp.mock.calls.some(([input]) =>
        input.url.includes("/prompt_async"),
      ),
    ).toBe(false);
    expect(killChild).toHaveBeenCalledWith("opencode-live");
  });

  it("opens a fresh stream after cancellation even if the old stream ends", async () => {
    const { done } = await startTurn([]);
    await cancelOpenCodeTurn("opencode-live");
    await done;
    onSseEnd?.();
    const next = turn([]);
    await waitFor(
      () => spawnChild.mock.calls.length === 2,
      "replacement server",
    );
    await waitFor(
      () =>
        harnessHttp.mock.calls.filter(([input]) =>
          input.url.includes("/prompt_async"),
        ).length === 2,
      "next prompt",
    );
    idle();
    await next;
  });

  it("serializes concurrent startup before running both queued prompts", async () => {
    const first = turn([]);
    const second = turn([]);
    await waitFor(() => promptMessageID !== undefined, "first prompt");
    expect(spawnChild).toHaveBeenCalledOnce();
    idle();
    await first;
    await waitFor(
      () =>
        harnessHttp.mock.calls.filter(([input]) =>
          input.url.includes("/prompt_async"),
        ).length === 2,
      "queued prompt",
    );
    idle();
    await second;
  });

  it("does not submit queued turns, compaction, or rewind after cancellation", async () => {
    const { done } = await startTurn([]);
    const queuedTurn = turn([]);
    const queuedCompaction = compactOpenCodeContext(sessionInput());
    const queuedRewind = rewindOpenCodeLastTurn(sessionInput());
    await new Promise((resolve) => setTimeout(resolve, 0));
    await cancelOpenCodeTurn("opencode-live");
    await Promise.all([done, queuedTurn, queuedCompaction, queuedRewind]);
    expect(
      harnessHttp.mock.calls.filter(([input]) =>
        input.url.includes("/prompt_async"),
      ),
    ).toHaveLength(1);
    expect(
      harnessHttp.mock.calls.some(([input]) =>
        /\/(summarize|revert)$/.test(new URL(input.url).pathname),
      ),
    ).toBe(false);
  });

  it("rejects a queued operation on an ended stream before it submits", async () => {
    const { done } = await startTurn([]);
    const first = done.catch((error) => error);
    const queued = turn([]).catch((error) => error);
    await new Promise((resolve) => setTimeout(resolve, 0));
    onSseEnd?.();
    expect(await first).toBeInstanceOf(Error);
    expect(await queued).toMatchObject({
      message:
        "OpenCode session ended before this operation could start. Retry the request.",
    });
    expect(
      harnessHttp.mock.calls.filter(([input]) =>
        input.url.includes("/prompt_async"),
      ),
    ).toHaveLength(1);
  });

  it("finishes invalidated stream cleanup before opening a replacement", async () => {
    const { done } = await startTurn([]);
    const first = done.catch((error) => error);
    let releaseKill!: () => void;
    killChild.mockImplementationOnce(
      () =>
        new Promise<undefined>((resolve) => {
          releaseKill = () => resolve(undefined);
        }),
    );
    onSseEnd?.();
    await waitFor(() => releaseKill !== undefined, "old process cleanup");
    const second = turn([]);
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(spawnChild).toHaveBeenCalledOnce();
    releaseKill();
    expect(await first).toBeInstanceOf(Error);
    await waitFor(
      () => spawnChild.mock.calls.length === 2,
      "replacement process",
    );
    await waitFor(
      () =>
        harnessHttp.mock.calls.filter(([input]) =>
          input.url.includes("/prompt_async"),
        ).length === 2,
      "replacement prompt",
    );
    idle();
    await second;
  });

  it("does not carry repeated idle cancellation into the next prompt", async () => {
    const { done } = await startTurn([]);
    await cancelOpenCodeTurn("opencode-live");
    await done;
    await cancelOpenCodeTurn("opencode-live");
    let settled = false;
    const next = turn([]).then(() => {
      settled = true;
    });
    await waitFor(
      () =>
        harnessHttp.mock.calls.filter(([input]) =>
          input.url.includes("/prompt_async"),
        ).length === 2,
      "next prompt",
    );
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(settled).toBe(false);
    idle();
    await next;
  });

  it.each([
    { id: "internal_compaction", parts: [{ type: "compaction", auto: false }] },
    {
      id: "internal_continue",
      parts: [{ type: "text", text: "Continue", synthetic: true }],
    },
  ])("rewinds the visible request before $id", async ({ id, parts }) => {
    sessionMessages = [
      {
        info: { id: "visible_user", role: "user", time: { created: 1 } },
        parts: [{ type: "text", text: "Original request" }],
      },
      { info: { id, role: "user", time: { created: 2 } }, parts },
    ];
    await rewindOpenCodeLastTurn(sessionInput());
    const request = harnessHttp.mock.calls.find(([input]) =>
      input.url.includes("/revert"),
    )?.[0];
    expect(JSON.parse(request!.body!)).toEqual({ messageID: "visible_user" });
  });

  it("clears rejected prompt state so rewind and later prompts still work", async () => {
    sessionMessages = [
      {
        info: { id: "previous_user", role: "user", time: { created: 1 } },
        parts: [{ type: "text", text: "Previous request" }],
      },
    ];
    harnessHttp.mockImplementation(async (input) =>
      input.url.includes("/prompt_async")
        ? { status: 500, body: "Prompt rejected" }
        : defaultHarnessHttp(input),
    );
    await expect(turn([])).rejects.toThrow("Prompt rejected");
    await expect(
      steerOpenCodeTurn({ ...sessionInput(), text: "Follow-up" }),
    ).rejects.toBeInstanceOf(TurnNotReadyError);
    await expect(rewindOpenCodeLastTurn(sessionInput())).resolves.toEqual({
      submitted: false,
    });
    harnessHttp.mockImplementation(defaultHarnessHttp);
    const next = turn([]);
    await waitFor(() => promptMessageID !== undefined, "recovered prompt");
    idle();
    await next;
  });

  it("applies final corrections to one part and ignores its late deltas", async () => {
    const events: HarnessEvent[] = [];
    const { done } = await startTurn(events);
    emitMessage({
      id: "assistant_text",
      role: "assistant",
      parentID: promptMessageID,
    });
    const part = (text: string, end?: number) =>
      onSseEvent?.({
        type: "message.part.updated",
        properties: {
          part: {
            id: "text_part",
            sessionID: "session_1",
            messageID: "assistant_text",
            type: "text",
            text,
            time: { start: 1, ...(end ? { end } : {}) },
          },
        },
      });
    part("Hello worle");
    part("Hello world", 2);
    onSseEvent?.({
      type: "message.part.delta",
      properties: {
        sessionID: "session_1",
        partID: "text_part",
        field: "text",
        delta: "ld",
      },
    });
    const session = events.reduce(
      applyHarnessEvent,
      newSession("opencode", "/repo"),
    );
    expect(
      session.blocks
        .filter((block) => block.role === "assistant")
        .map((block) => block.text),
    ).toEqual(["Hello world"]);
    part("Hello", 3);
    expect(
      events
        .reduce(applyHarnessEvent, session)
        .blocks.find((block) => block.providerPartId === "text_part")?.text,
    ).toBe("Hello");
    idle();
    await done;
  });

  it("waits through recoverable context overflow and returns its resumed answer", async () => {
    const events: HarnessEvent[] = [];
    const { done } = await startTurn(events);
    let settled = false;
    const observed = done.then(() => {
      settled = true;
    });
    onSseEvent?.({
      type: "session.error",
      properties: {
        sessionID: "session_1",
        error: {
          name: "ContextOverflowError",
          data: { message: "Context exceeded" },
        },
      },
    });
    emitMessage({ id: "compact_user", role: "user" }, [
      { type: "compaction", auto: true },
    ]);
    emitMessage({
      id: "summary",
      role: "assistant",
      agent: "compaction",
      parentID: "compact_user",
      finish: "stop",
    });
    emitMessage(
      { id: "replayed_user", role: "user" },
      sessionMessages[0].parts,
    );
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(settled).toBe(false);
    expect(events.some((event) => event.type === "session.error")).toBe(false);
    emitMessage({
      id: "recovered",
      role: "assistant",
      agent: "build",
      parentID: "replayed_user",
      finish: "stop",
    });
    onSseEvent?.({
      type: "message.part.updated",
      properties: {
        part: {
          id: "recovered_text",
          sessionID: "session_1",
          messageID: "recovered",
          type: "text",
          text: "Recovered after compaction",
          time: { start: 1, end: 2 },
        },
      },
    });
    fakeSessionStatus = "idle";
    rawIdle();
    await observed;
    expect(
      events
        .reduce(applyHarnessEvent, newSession("opencode", "/repo"))
        .blocks.some((block) => block.text === "Recovered after compaction"),
    ).toBe(true);
  });

  it("reports context overflow when the compaction itself fails", async () => {
    const events: HarnessEvent[] = [];
    const { done } = await startTurn(events);
    onSseEvent?.({
      type: "session.error",
      properties: {
        sessionID: "session_1",
        error: {
          name: "ContextOverflowError",
          data: { message: "Context exceeded" },
        },
      },
    });
    emitMessage({ id: "compact_user", role: "user" }, [
      { type: "compaction", auto: true },
    ]);
    emitMessage({
      id: "summary",
      role: "assistant",
      agent: "compaction",
      parentID: "compact_user",
      error: {
        name: "ContextOverflowError",
        data: { message: "Too large to compact" },
      },
    });
    fakeSessionStatus = "idle";
    rawIdle();
    await done;
    expect(events).toContainEqual({
      type: "session.error",
      message: "Too large to compact",
    });
  });

  it("ignores late users and unrelated newer users when checking idle", async () => {
    const { done } = await startTurn([]);
    let settled = false;
    const observed = done.then(() => {
      settled = true;
    });
    emitMessage({ id: "old_user", role: "user", time: { created: 1 } }, [
      { type: "text", text: "Old request" },
    ]);
    emitMessage({
      id: "old_reply",
      role: "assistant",
      parentID: "old_user",
      agent: "build",
      finish: "stop",
    });
    emitMessage({ id: "unrelated_user", role: "user" }, [
      { type: "text", text: "Unrelated request" },
    ]);
    emitMessage({
      id: "unrelated_reply",
      role: "assistant",
      parentID: "unrelated_user",
      agent: "build",
      finish: "stop",
    });
    fakeSessionStatus = "idle";
    rawIdle();
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(settled).toBe(false);
    idle();
    await observed;
  });

  it("rechecks an idle that arrives during an older status request", async () => {
    const { done } = await startTurn([]);
    let releaseStatus!: (value: { status: number; body: string }) => void;
    const staleStatus = new Promise<{ status: number; body: string }>(
      (resolve) => {
        releaseStatus = resolve;
      },
    );
    harnessHttp.mockImplementationOnce(() => staleStatus);
    rawIdle();
    await new Promise((resolve) => setTimeout(resolve, 0));
    idle();
    releaseStatus({
      status: 200,
      body: JSON.stringify({ session_1: { type: "busy" } }),
    });
    await done;
    expect(
      harnessHttp.mock.calls.filter(
        ([input]) => new URL(input.url).pathname === "/session/status",
      ),
    ).toHaveLength(2);
  });

  it("waits for the latest owned steering prompt at idle", async () => {
    const { done } = await startTurn([]);
    const original = promptMessageID;
    await steerOpenCodeTurn({
      ...sessionInput(),
      text: "Current steering request",
    });
    let settled = false;
    const observed = done.then(() => {
      settled = true;
    });
    emitMessage({
      id: "first_reply",
      role: "assistant",
      parentID: original,
      agent: "build",
      finish: "stop",
    });
    fakeSessionStatus = "idle";
    rawIdle();
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(settled).toBe(false);
    idle();
    await observed;
  });

  it("correlates a synthetic continuation only after its current compaction", async () => {
    const events: HarnessEvent[] = [];
    const { done } = await startTurn(events);
    emitMessage({ id: "compact_user", role: "user" }, [
      { type: "compaction", auto: true },
    ]);
    emitMessage({
      id: "summary",
      role: "assistant",
      parentID: "compact_user",
      agent: "compaction",
      finish: "stop",
    });
    emitMessage({ id: "continue_user", role: "user" }, [
      {
        type: "text",
        text: "Continue",
        synthetic: true,
        metadata: { compaction_continue: true },
      },
    ]);
    emitMessage({
      id: "final_reply",
      role: "assistant",
      parentID: "continue_user",
      agent: "build",
      finish: "stop",
    });
    fakeSessionStatus = "idle";
    rawIdle();
    await done;
    expect(events).toContainEqual({ type: "message.completed" });
  });

  it("does not use a delayed manual compaction idle to complete a new prompt", async () => {
    await compactOpenCodeContext(sessionInput());
    let settled = false;
    const next = turn([]).then(() => {
      settled = true;
    });
    await waitFor(() => promptMessageID !== undefined, "new prompt");
    rawIdle();
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(settled).toBe(false);
    idle();
    await next;
  });

  it("fails manual compaction on a durable summary error", async () => {
    harnessHttp.mockImplementation(async (input) => {
      if (input.url.includes("/summarize"))
        sessionMessages.push({
          info: {
            id: "failed_summary",
            role: "assistant",
            agent: "compaction",
            error: { data: { message: "Summary failed" } },
          },
        });
      return defaultHarnessHttp(input);
    });
    await expect(compactOpenCodeContext(sessionInput())).rejects.toThrow(
      "Summary failed",
    );
  });

  it("keeps a failed attachment read warning nonfatal through a successful reply", async () => {
    const events: HarnessEvent[] = [];
    harnessHttp.mockImplementation(async (input) => {
      if (input.url.includes("/prompt_async"))
        onSseEvent?.({
          type: "session.error",
          properties: {
            sessionID: "session_1",
            error: {
              name: "UnknownError",
              data: { message: "ENOENT: no such file" },
            },
          },
        });
      return defaultHarnessHttp(input);
    });
    const { done } = await startTurn(events);
    expect(events).toContainEqual({
      type: "status",
      text: "ENOENT: no such file",
    });
    expect(events.some((event) => event.type === "session.error")).toBe(false);
    idle();
    await done;
    expect(events.some((event) => event.type === "session.error")).toBe(false);
  });

  it("reports the current project's custom model context window", async () => {
    const model = {
      id: "opencode:openai/review",
      harness: "opencode" as const,
      name: "Review",
      contextWindow: 100_000,
    };
    setHarnessModels("opencode", [model]);
    setProjectHarnessModels("opencode", "/repo", [
      { ...model, contextWindow: 8_192 },
    ]);
    const events: HarnessEvent[] = [];
    const { done } = await startTurn(events);
    emitMessage({
      id: "context_reply",
      role: "assistant",
      parentID: promptMessageID,
      providerID: "openai",
      modelID: "review",
      tokens: { input: 250, output: 20, cache: { read: 0, write: 0 } },
    });
    expect(events).toContainEqual({
      type: "context",
      used: 270,
      window: 8_192,
    });
    idle();
    await done;
  });

  it.each(["Agent not found: missing", "Model not found: local/missing"])(
    "reports terminal setup failure %s without waiting for a user or idle event",
    async (message) => {
      vi.useFakeTimers();
      const events: HarnessEvent[] = [];
      harnessHttp.mockImplementation(async (input) => {
        if (input.url.includes("/prompt_async")) {
          onSseEvent?.({
            type: "session.error",
            properties: {
              sessionID: "session_1",
              error: { name: "UnknownError", data: { message } },
            },
          });
          return { status: 204, body: "" };
        }
        return defaultHarnessHttp(input);
      });
      const done = turn(events);
      await vi.advanceTimersByTimeAsync(50);
      await done;
      expect(events).toContainEqual({ type: "session.error", message });
      harnessHttp.mockImplementation(defaultHarnessHttp);
      const next = turn([]);
      await vi.advanceTimersByTimeAsync(0);
      idle();
      await next;
    },
  );

  it("bounds an anonymous preparation error with no durable progress", async () => {
    vi.useFakeTimers();
    const events: HarnessEvent[] = [];
    harnessHttp.mockImplementation(async (input) => {
      if (input.url.includes("/prompt_async")) {
        onSseEvent?.({
          type: "session.error",
          properties: {
            sessionID: "session_1",
            error: {
              name: "UnknownError",
              data: { message: "Preparation hook failed" },
            },
          },
        });
        return { status: 204, body: "" };
      }
      return defaultHarnessHttp(input);
    });
    let settled = false;
    const done = turn(events).then(() => {
      settled = true;
    });
    await vi.advanceTimersByTimeAsync(29_999);
    expect(settled).toBe(false);
    await vi.advanceTimersByTimeAsync(1);
    await done;
    expect(events).toContainEqual({
      type: "session.error",
      message: "Preparation hook failed",
    });
    harnessHttp.mockImplementation(defaultHarnessHttp);
    const next = turn([]);
    await vi.advanceTimersByTimeAsync(0);
    idle();
    await next;
  });

  it("does not classify an attachment warning while its assistant is still running", async () => {
    const events: HarnessEvent[] = [];
    const { done } = await startTurn(events);
    vi.useFakeTimers();
    onSseEvent?.({
      type: "session.error",
      properties: {
        sessionID: "session_1",
        error: {
          name: "UnknownError",
          data: { message: "Attachment read failed" },
        },
      },
    });
    emitMessage({
      id: "running_reply",
      role: "assistant",
      parentID: promptMessageID,
      agent: "build",
    });
    fakeSessionStatus = "idle";
    rawIdle();
    await vi.advanceTimersByTimeAsync(60_000);
    expect(events.some((event) => event.type === "session.error")).toBe(false);
    idle();
    await done;
  });

  it("does not extend a preparation failure grace for old unrelated transcript updates", async () => {
    vi.useFakeTimers();
    const events: HarnessEvent[] = [];
    harnessHttp.mockImplementation(async (input) =>
      input.url.includes("/prompt_async")
        ? { status: 204, body: "" }
        : defaultHarnessHttp(input),
    );
    const done = turn(events);
    await vi.advanceTimersByTimeAsync(0);
    onSseEvent?.({
      type: "session.error",
      properties: {
        sessionID: "session_1",
        error: {
          name: "UnknownError",
          data: { message: "Preparation failed" },
        },
      },
    });
    await vi.advanceTimersByTimeAsync(20_000);
    emitMessage({ id: "old_user", role: "user", time: { created: 1 } });
    emitMessage({
      id: "old_assistant",
      role: "assistant",
      parentID: "old_user",
    });
    onSseEvent?.({
      type: "message.part.updated",
      properties: {
        part: {
          sessionID: "session_1",
          id: "old_part",
          messageID: "old_assistant",
          type: "text",
          text: "Unrelated update",
        },
      },
    });
    await vi.advanceTimersByTimeAsync(10_000);
    await done;
    expect(events).toContainEqual({
      type: "session.error",
      message: "Preparation failed",
    });
  });

  it("does not classify an old error after steering changes the stable-idle snapshot", async () => {
    const events: HarnessEvent[] = [];
    const { done } = await startTurn(events);
    vi.useFakeTimers();
    let reads = 0;
    let releaseStatus!: (value: { status: number; body: string }) => void;
    harnessHttp.mockImplementation(async (input) => {
      if (new URL(input.url).pathname === "/session/status" && ++reads === 3)
        return new Promise((resolve) => {
          releaseStatus = resolve;
        });
      return defaultHarnessHttp(input);
    });
    fakeSessionStatus = "idle";
    onSseEvent?.({
      type: "session.error",
      properties: {
        sessionID: "session_1",
        error: {
          name: "UnknownError",
          data: { message: "Preparation failed" },
        },
      },
    });
    await vi.advanceTimersByTimeAsync(30_000);
    expect(releaseStatus).toBeTypeOf("function");
    await steerOpenCodeTurn({ ...sessionInput(), text: "Corrected request" });
    releaseStatus({
      status: 200,
      body: JSON.stringify({ session_1: { type: "idle" } }),
    });
    await vi.advanceTimersByTimeAsync(0);
    expect(events.some((event) => event.type === "session.error")).toBe(false);
    idle();
    await done;
  });

  it("refreshes the warning grace when steering arrives before its user event", async () => {
    const events: HarnessEvent[] = [];
    const { done } = await startTurn(events);
    vi.useFakeTimers();
    let releaseMessages!: (value: { status: number; body: string }) => void;
    let paused = false;
    harnessHttp.mockImplementation(async (input) => {
      if (
        new URL(input.url).pathname === "/session/session_1/message" &&
        !paused
      ) {
        paused = true;
        return new Promise((resolve) => {
          releaseMessages = resolve;
        });
      }
      if (input.url.includes("/prompt_async")) return { status: 204, body: "" };
      return defaultHarnessHttp(input);
    });
    fakeSessionStatus = "idle";
    onSseEvent?.({
      type: "session.error",
      properties: {
        sessionID: "session_1",
        error: {
          name: "UnknownError",
          data: { message: "Attachment read warning" },
        },
      },
    });
    await vi.advanceTimersByTimeAsync(30_000);
    expect(releaseMessages).toBeTypeOf("function");
    await steerOpenCodeTurn({ ...sessionInput(), text: "Corrected request" });
    const requests = harnessHttp.mock.calls.filter(([input]) =>
      input.url.includes("/prompt_async"),
    );
    const messageID = JSON.parse(
      requests[requests.length - 1][0].body!,
    ).messageID;
    releaseMessages({ status: 200, body: JSON.stringify(sessionMessages) });
    await vi.advanceTimersByTimeAsync(0);
    expect(events.some((event) => event.type === "session.error")).toBe(false);
    harnessHttp.mockImplementation(defaultHarnessHttp);
    promptMessageID = messageID;
    emitMessage(
      { id: messageID, role: "user", time: { created: Date.now() } },
      [{ type: "text", text: "Corrected request" }],
    );
    idle();
    await done;
  });
});
