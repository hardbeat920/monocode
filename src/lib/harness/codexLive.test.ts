import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const sent: string[] = [];
let onLine: ((line: string) => void) | undefined;
const writeChild = vi.fn(async (_id: string, line: string) => {
  sent.push(line);
});

vi.mock("./child", () => ({
  resolveCodexBinary: async () => ({ path: "/fake/codex" }),
  spawnChild: async () => undefined,
  killChild: async () => undefined,
  unwatchChild: () => undefined,
  watchChild: (_id: string, line: (l: string) => void) => {
    onLine = line;
  },
  writeChild,
}));

const {
  compactCodexContext,
  bindCodexSession,
  cancelCodexTurn,
  keepCodexQuestionOpen,
  respondCodexApproval,
  respondCodexQuestion,
  sendCodexTurn,
  stopCodexSession,
  __codexTestReset,
} = await import("./codex");
import type { HarnessEvent } from "./types";
import { newSession, type RuntimeMode, type TurnIntent } from "../session";
import { applyHarnessEvent } from "./apply";

function parse() {
  return sent.map((line) => JSON.parse(line) as Record<string, unknown>);
}

function reply(id: number, result: unknown) {
  onLine!(JSON.stringify({ id, result }));
}

function notify(method: string, params: unknown) {
  onLine!(JSON.stringify({ method, params }));
}

const waitFor = async (pred: () => boolean, label: string) => {
  for (let i = 0; i < 200; i++) {
    if (pred()) return;
    await new Promise((r) => setTimeout(r, 5));
  }
  throw new Error(
    `timed out waiting for ${label}; sent=${JSON.stringify(parse().map((m) => m.method ?? `reply:${m.id}`))}`,
  );
};

async function startTurn(
  sessionId: string,
  options: {
    runtimeMode?: RuntimeMode;
    intent?: TurnIntent;
    resume?: boolean;
    beforeThreadReply?: () => Promise<void>;
  } = {},
) {
  const events: HarnessEvent[] = [];
  if (options.resume) bindCodexSession(sessionId, "thr_1", "/repo");
  const turn = sendCodexTurn({
    sessionId,
    cwd: "/repo",
    model: "codex:gpt-5.4",
    modelSettings: {},
    runtimeMode: options.runtimeMode ?? "supervised",
    intent: options.intent,
    text: "summarize the changelog",
    attachments: [],
    onEvent: (event) => events.push(event),
  });

  await waitFor(
    () => parse().some((m) => m.method === "initialize"),
    "initialize",
  );
  reply(parse().find((m) => m.method === "initialize")!.id as number, {});
  const threadMethod = options.resume ? "thread/resume" : "thread/start";
  await waitFor(
    () => parse().some((m) => m.method === threadMethod),
    threadMethod,
  );
  await options.beforeThreadReply?.();
  reply(parse().find((m) => m.method === threadMethod)!.id as number, {
    thread: { id: "thr_1" },
  });

  await waitFor(
    () => parse().some((m) => m.method === "turn/start"),
    "turn/start",
  );
  reply(parse().find((m) => m.method === "turn/start")!.id as number, {
    turn: { id: "turn_1", status: "inProgress" },
  });
  notify("turn/started", { turn: { id: "turn_1", status: "inProgress" } });
  return { events, turn };
}

describe("codex live turn sequence", () => {
  beforeEach(() => {
    sent.length = 0;
    onLine = undefined;
    writeChild.mockClear();
  });

  afterEach(async () => {
    vi.useRealTimers();
    vi.restoreAllMocks();
    await stopCodexSession("codex-live");
    __codexTestReset();
  });

  it("keeps retries and HTTP fallback out of a successful turn's transcript", async () => {
    const debug = vi.spyOn(console, "debug").mockImplementation(() => {});
    const { events, turn } = await startTurn("codex-live");
    const settled = vi.fn();
    void turn.then(settled);
    const beforeRetries = [...events];
    for (let attempt = 1; attempt <= 5; attempt++) {
      notify("error", {
        threadId: "thr_1",
        turnId: "turn_1",
        error: { message: `Reconnecting... ${attempt}/5` },
        willRetry: true,
      });
    }
    const fallback =
      "Falling back from WebSockets to HTTPS transport. unexpected status 404 Not Found: Unknown endpoint: GET /v1/responses, url: ws://127.0.0.1:19101/v1/responses";
    notify("warning", { threadId: "thr_1", message: fallback });
    await Promise.resolve();
    expect(settled).not.toHaveBeenCalled();
    expect(events).toEqual(beforeRetries);
    expect(debug).toHaveBeenCalledTimes(6);
    expect(debug).toHaveBeenCalledWith(expect.any(String), fallback);

    notify("item/agentMessage/delta", { delta: "The answer" });
    notify("turn/completed", { turn: { id: "turn_1", status: "completed" } });
    await turn;
    expect(settled).toHaveBeenCalledOnce();
    const session = events.reduce(
      applyHarnessEvent,
      newSession("codex", "/repo", "codex:gpt-5.4", "supervised"),
    );
    expect(session.blocks).toMatchObject([
      { role: "assistant", text: "The answer", streaming: false },
    ]);
  });

  it("still surfaces a terminal failure after transport retries", async () => {
    vi.spyOn(console, "debug").mockImplementation(() => {});
    const { events, turn } = await startTurn("codex-live");
    notify("error", {
      error: { message: "Reconnecting... 5/5" },
      willRetry: true,
    });
    const message =
      "Response stream disconnected after too many failed attempts";
    notify("error", { error: { message }, willRetry: false });
    expect(events).toContainEqual({ type: "session.error", message });
    notify("turn/completed", {
      turn: { id: "turn_1", status: "failed", error: { message } },
    });
    await turn;
    const session = events.reduce(
      applyHarnessEvent,
      newSession("codex", "/repo", "codex:gpt-5.4", "supervised"),
    );
    expect(session.blocks).toContainEqual(
      expect.objectContaining({ role: "system", text: message }),
    );
    expect(
      session.blocks.some((block) => block.text.includes("Reconnecting")),
    ).toBe(false);
  });

  it.each([false, true])(
    "answers the external clock before thread setup finishes, resume=%s",
    async (resume) => {
      vi.spyOn(Date, "now").mockReturnValue(1_789_000_000_789);
      const { events, turn } = await startTurn("codex-live", {
        resume,
        beforeThreadReply: async () => {
          onLine!(
            JSON.stringify({
              id: "clock_setup",
              method: "currentTime/read",
              params: { threadId: "thr_1" },
            }),
          );
          await Promise.resolve();
        },
      });
      notify("turn/completed", { turn: { id: "turn_1", status: "completed" } });
      await turn;
      expect(parse().find((m) => m.id === "clock_setup")).toEqual({
        id: "clock_setup",
        result: { currentTimeAt: 1_789_000_000 },
      });
      expect(events.some((e) => e.type === "session.error")).toBe(false);
    },
  );

  it.each([undefined, "plan"] as const)(
    "answers fresh clock reads without interrupting a pending question, intent=%s",
    async (intent) => {
      const now = vi.spyOn(Date, "now").mockReturnValue(1_789_000_000_789);
      const { events, turn } = await startTurn("codex-live", { intent });
      expect(
        parse().find((m) => m.method === "initialize")?.params,
      ).toMatchObject({
        capabilities: { experimentalApi: true },
      });
      expect(
        parse().find((m) => m.method === "turn/start")?.params,
      ).toMatchObject({
        collaborationMode: { mode: intent === "plan" ? "plan" : "default" },
      });
      onLine!(
        JSON.stringify({
          id: "pending_question",
          method: "item/tool/requestUserInput",
          params: {
            itemId: "q1",
            questions: [
              { id: "choice", header: "Source", question: "Which source?" },
            ],
          },
        }),
      );
      await waitFor(
        () => events.some((e) => e.type === "question.asked"),
        "question",
      );
      const beforeClock = [...events];
      for (const [id, millis] of [
        [91, 1_789_000_000_789],
        ["clock_next", 1_789_000_005_123],
      ] as const) {
        now.mockReturnValue(millis);
        onLine!(
          JSON.stringify({
            id,
            method: "currentTime/read",
            params: { threadId: "thr_1" },
          }),
        );
        await waitFor(
          () => parse().some((m) => m.id === id),
          "external clock reply",
        );
        expect(parse().find((m) => m.id === id)).toEqual({
          id,
          result: { currentTimeAt: Math.floor(millis / 1000) },
        });
      }
      expect(events).toEqual(beforeClock);
      expect(parse().some((m) => m.id === "pending_question")).toBe(false);
      notify("turn/completed", { turn: { id: "turn_1", status: "completed" } });
      await turn;
    },
  );

  it.each([false, true])(
    "routes full-access escalation after resume=%s",
    async (resume) => {
      const { events, turn } = await startTurn("codex-live", {
        runtimeMode: "full-access",
        resume,
      });
      for (const message of parse().filter((m) =>
        ["thread/start", "thread/resume", "turn/start"].includes(
          String(m.method),
        ),
      )) {
        expect(message.params).toMatchObject({
          approvalPolicy: "on-request",
          approvalsReviewer: "user",
        });
      }
      onLine!(
        JSON.stringify({
          id: 91,
          method: "item/commandExecution/requestApproval",
          params: { itemId: "read_1", command: "git status --short" },
        }),
      );
      await waitFor(
        () => parse().some((m) => m.id === 91),
        "full-access response",
      );
      expect(parse().find((m) => m.id === 91)?.result).toEqual({
        decision: "accept",
      });
      expect(events.some((e) => e.type === "approval.requested")).toBe(false);
      notify("turn/completed", { turn: { id: "turn_1", status: "completed" } });
      await turn;
    },
  );

  it("still waits for an explicit command decision in supervised mode", async () => {
    const { events, turn } = await startTurn("codex-live");
    onLine!(
      JSON.stringify({
        id: 91,
        method: "item/commandExecution/requestApproval",
        params: { itemId: "cmd_1", command: "git status --short" },
      }),
    );
    await waitFor(
      () => events.some((e) => e.type === "approval.requested"),
      "approval UI",
    );
    expect(parse().some((m) => m.id === 91)).toBe(false);
    const request = events.find((e) => e.type === "approval.requested")!;
    if (request.type !== "approval.requested")
      throw new Error("missing approval");
    respondCodexApproval("codex-live", request.requestId, "deny");
    await waitFor(() => parse().some((m) => m.id === 91), "denial");
    expect(parse().find((m) => m.id === 91)?.result).toEqual({
      decision: "decline",
    });
    notify("turn/completed", { turn: { id: "turn_1", status: "completed" } });
    await turn;
  });

  it.each(["allow", "deny"] as const)(
    "keeps a child approval answerable after a sibling completes: %s",
    async (decision) => {
      const { events, turn } = await startTurn("codex-live");
      const settled = vi.fn();
      void turn.then(settled);
      onLine!(
        JSON.stringify({
          id: "child_approval",
          method: "item/commandExecution/requestApproval",
          params: {
            threadId: "thr_child",
            turnId: "turn_child",
            itemId: "child_read",
            command: "cat ~/.gitconfig",
          },
        }),
      );
      const approval = events.find(
        (event) => event.type === "approval.requested",
      )!;
      expect(approval).toMatchObject({ callId: "child_read" });
      const before = [...events];
      notify("turn/started", {
        threadId: "thr_sibling",
        turn: { id: "turn_sibling" },
      });
      notify("item/agentMessage/delta", {
        threadId: "thr_sibling",
        delta: "Child-only text",
      });
      notify("turn/completed", {
        threadId: "thr_sibling",
        turn: { id: "turn_sibling", status: "completed" },
      });
      notify("error", {
        threadId: "thr_sibling",
        error: { message: "Child failed" },
        willRetry: false,
      });
      await Promise.resolve();
      expect(events).toEqual(before);
      expect(settled).not.toHaveBeenCalled();
      respondCodexApproval("codex-live", approval.requestId, decision);
      await waitFor(
        () => parse().some((message) => message.id === "child_approval"),
        "child decision",
      );
      expect(
        parse().find((message) => message.id === "child_approval")?.result,
      ).toEqual({
        decision: decision === "allow" ? "accept" : "decline",
      });
      notify("turn/completed", {
        threadId: "thr_1",
        turn: { id: "turn_1", status: "completed" },
      });
      await turn;
    },
  );

  it("clears a server-resolved child approval using its owning thread", async () => {
    const { events, turn } = await startTurn("codex-live");
    onLine!(
      JSON.stringify({
        id: "child_approval",
        method: "item/commandExecution/requestApproval",
        params: {
          threadId: "thr_child",
          itemId: "child_read",
          command: "cat ~/.gitconfig",
        },
      }),
    );
    const approval = events.find(
      (event) => event.type === "approval.requested",
    )!;
    notify("serverRequest/resolved", {
      threadId: "thr_1",
      requestId: "child_approval",
    });
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(events.some((event) => event.type === "approval.resolved")).toBe(
      false,
    );
    notify("serverRequest/resolved", {
      threadId: "thr_child",
      requestId: "child_approval",
    });
    await waitFor(
      () => events.some((event) => event.type === "approval.resolved"),
      "child cleanup",
    );
    expect(events).toContainEqual({
      type: "approval.resolved",
      requestId: approval.requestId,
      decision: "cancelled",
    });
    expect(parse().some((message) => message.id === "child_approval")).toBe(
      false,
    );
    notify("turn/completed", {
      threadId: "thr_1",
      turn: { id: "turn_1", status: "completed" },
    });
    await turn;
  });

  it.each(["allow", "deny"] as const)(
    "answers a child's filesystem permission request: %s",
    async (decision) => {
      const { events, turn } = await startTurn("codex-live");
      const permissions = { fileSystem: { read: ["/home/user/.gitconfig"] } };
      onLine!(
        JSON.stringify({
          id: "child_permissions",
          method: "item/permissions/requestApproval",
          params: {
            threadId: "thr_child",
            turnId: "turn_child",
            itemId: "child_read",
            permissions,
          },
        }),
      );
      const approval = events.find(
        (event) => event.type === "approval.requested",
      )!;
      respondCodexApproval("codex-live", approval.requestId, decision);
      await waitFor(
        () => parse().some((message) => message.id === "child_permissions"),
        "filesystem permission response",
      );
      expect(
        parse().find((message) => message.id === "child_permissions")?.result,
      ).toEqual(
        decision === "allow"
          ? { scope: "turn", permissions }
          : { permissions: {} },
      );
      notify("turn/completed", {
        threadId: "thr_1",
        turn: { id: "turn_1", status: "completed" },
      });
      await turn;
    },
  );

  it("advances the question queue when the server resolves a child's request", async () => {
    const { events, turn } = await startTurn("codex-live");
    for (const id of ["child_a", "child_b"]) {
      onLine!(
        JSON.stringify({
          id,
          method: "item/tool/requestUserInput",
          params: {
            threadId: id,
            questions: [{ id: "q", question: id, isOther: true, options: [] }],
          },
        }),
      );
    }
    notify("serverRequest/resolved", {
      threadId: "child_a",
      requestId: "child_a",
    });
    await waitFor(
      () =>
        events.filter((event) => event.type === "question.asked").length === 2,
      "second child question",
    );
    const session = events.reduce(
      applyHarnessEvent,
      newSession("codex", "/repo"),
    );
    expect(session.pendingQuestion?.questions[0].prompt).toBe("child_b");
    expect(parse().some((message) => message.id === "child_a")).toBe(false);
    respondCodexQuestion("codex-live", session.pendingQuestion!.requestId, {
      kind: "skipped",
    });
    await waitFor(
      () => parse().some((message) => message.id === "child_b"),
      "second child response",
    );
    notify("turn/completed", {
      threadId: "thr_1",
      turn: { id: "turn_1", status: "completed" },
    });
    await turn;
  });

  it("fails the active turn if a child permission reply cannot be delivered", async () => {
    const { events, turn } = await startTurn("codex-live");
    onLine!(
      JSON.stringify({
        id: "child_approval",
        method: "item/commandExecution/requestApproval",
        params: {
          threadId: "thr_child",
          itemId: "child_read",
          command: "cat ~/.gitconfig",
        },
      }),
    );
    const approval = events.find(
      (event) => event.type === "approval.requested",
    )!;
    let outcome: unknown;
    void turn.catch((error) => {
      outcome = error;
    });
    writeChild.mockRejectedValueOnce(new Error("Broken pipe"));
    respondCodexApproval("codex-live", approval.requestId, "allow");
    await waitFor(() => outcome instanceof Error, "failed permission delivery");
    expect(outcome).toMatchObject({ message: "Broken pipe" });
    expect(events).toContainEqual({
      type: "session.error",
      message: "Broken pipe",
    });
  });

  it.each([undefined, "plan"] as const)(
    "waits for user input in full-access intent=%s",
    async (intent) => {
      const { events, turn } = await startTurn("codex-live", {
        runtimeMode: "full-access",
        intent,
      });
      onLine!(
        JSON.stringify({
          id: "question_rpc",
          method: "item/tool/requestUserInput",
          params: {
            itemId: "question_1",
            questions: [
              {
                id: "permission",
                header: "Access",
                question: "Read the external source?",
                isOther: true,
                isSecret: false,
                options: [
                  { label: "Accept", description: "Read the source." },
                  { label: "Decline", description: "Skip." },
                ],
              },
            ],
          },
        }),
      );
      await waitFor(
        () => events.some((e) => e.type === "question.asked"),
        "question UI",
      );
      expect(parse().some((m) => m.id === "question_rpc")).toBe(false);
      const request = events.find((e) => e.type === "question.asked")!;
      if (request.type !== "question.asked")
        throw new Error("missing question");
      respondCodexQuestion("codex-live", request.requestId, {
        kind: "answered",
        answers: { permission: ["Decline"] },
      });
      await waitFor(
        () => parse().some((m) => m.id === "question_rpc"),
        "question response",
      );
      expect(parse().find((m) => m.id === "question_rpc")?.result).toEqual({
        answers: { permission: { answers: ["Decline"] } },
      });
      expect(events).toContainEqual({
        type: "question.resolved",
        requestId: request.requestId,
        decision: "answered",
      });
      notify("turn/completed", { turn: { id: "turn_1", status: "completed" } });
      await turn;
    },
  );

  it.each(["skip", "server", "complete", "stop", "cancel"])(
    "clears pending questions on %s",
    async (action) => {
      const { events, turn } = await startTurn("codex-live");
      onLine!(
        JSON.stringify({
          id: 91,
          method: "item/tool/requestUserInput",
          params: {
            itemId: "q1",
            questions: [
              {
                id: "q",
                question: "Which source?",
                isSecret: false,
                isOther: true,
                options: null,
              },
            ],
          },
        }),
      );
      await waitFor(
        () => events.some((e) => e.type === "question.asked"),
        "question UI",
      );
      const request = events.find((e) => e.type === "question.asked")!;
      if (request.type !== "question.asked")
        throw new Error("missing question");
      if (action === "skip")
        respondCodexQuestion("codex-live", request.requestId, {
          kind: "skipped",
        });
      if (action === "server")
        notify("serverRequest/resolved", { threadId: "thr_1", requestId: 91 });
      if (action === "complete")
        notify("turn/completed", {
          turn: { id: "turn_1", status: "completed" },
        });
      if (action === "stop") await stopCodexSession("codex-live");
      if (action === "cancel") {
        const cancelled = cancelCodexTurn("codex-live");
        await waitFor(
          () => parse().some((m) => m.method === "turn/interrupt"),
          "interrupt",
        );
        reply(
          parse().find((m) => m.method === "turn/interrupt")!.id as number,
          {},
        );
        await cancelled;
      }
      await waitFor(
        () => events.some((e) => e.type === "question.resolved"),
        "question cleanup",
      );
      expect(events).toContainEqual({
        type: "question.resolved",
        requestId: request.requestId,
        decision: action === "skip" ? "skipped" : "cancelled",
      });
      if (action === "skip")
        expect(parse().find((m) => m.id === 91)?.result).toEqual({
          answers: {},
        });
      else expect(parse().some((m) => m.id === 91)).toBe(false);
      respondCodexQuestion("codex-live", request.requestId, {
        kind: "answered",
        answers: {},
        custom: { q: "too late" },
      });
      notify("turn/completed", { turn: { id: "turn_1", status: "completed" } });
      await turn;
      expect(parse().filter((m) => m.id === 91)).toHaveLength(
        action === "skip" ? 1 : 0,
      );
    },
  );

  it("does not collect secret answers in the transcript question UI", async () => {
    const { events, turn } = await startTurn("codex-live");
    onLine!(
      JSON.stringify({
        id: 91,
        method: "item/tool/requestUserInput",
        params: {
          questions: [
            {
              id: "secret",
              question: "Enter a secret",
              isSecret: true,
              options: null,
            },
          ],
        },
      }),
    );
    await waitFor(
      () => parse().some((m) => m.id === 91),
      "unsupported secret response",
    );
    expect(events.some((e) => e.type === "question.asked")).toBe(false);
    expect(events).toContainEqual({
      type: "status",
      text: expect.stringContaining("secret input"),
    });
    expect(parse().find((m) => m.id === 91)?.result).toEqual({ answers: {} });
    notify("turn/completed", { turn: { id: "turn_1", status: "completed" } });
    await turn;
  });

  it("auto-approves full-access file and permission requests", async () => {
    const { events, turn } = await startTurn("codex-live", {
      runtimeMode: "full-access",
    });
    onLine!(
      JSON.stringify({
        id: 91,
        method: "item/fileChange/requestApproval",
        params: { itemId: "edit_1", reason: "Edit the requested file" },
      }),
    );
    const permissions = { network: { enabled: true } };
    onLine!(
      JSON.stringify({
        id: 92,
        method: "item/permissions/requestApproval",
        params: { itemId: "perm_1", permissions },
      }),
    );
    await waitFor(() => parse().some((m) => m.id === 92), "permission grant");
    expect(parse().find((m) => m.id === 91)?.result).toEqual({
      decision: "accept",
    });
    expect(parse().find((m) => m.id === 92)?.result).toEqual({
      scope: "session",
      permissions,
    });
    expect(events.some((e) => e.type === "approval.requested")).toBe(false);
    notify("turn/completed", { turn: { id: "turn_1", status: "completed" } });
    await turn;
  });

  it("queues concurrent questions instead of hiding the first one", async () => {
    const { events, turn } = await startTurn("codex-live");
    for (const id of [91, 92])
      onLine!(
        JSON.stringify({
          id,
          method: "item/tool/requestUserInput",
          params: {
            questions: [
              {
                id: `q${id}`,
                question: `Question ${id}`,
                options: null,
                isOther: true,
              },
            ],
          },
        }),
      );
    const asked = () => events.filter((e) => e.type === "question.asked");
    expect(asked()).toHaveLength(1);
    respondCodexQuestion("codex-live", asked()[0].requestId, {
      kind: "answered",
      answers: {},
      custom: { q91: "first answer" },
    });
    await waitFor(() => asked().length === 2, "second question");
    respondCodexQuestion("codex-live", asked()[1].requestId, {
      kind: "skipped",
    });
    await waitFor(() => parse().some((m) => m.id === 92), "second reply");
    expect(parse().find((m) => m.id === 91)?.result).toEqual({
      answers: { q91: { answers: ["first answer"] } },
    });
    expect(parse().find((m) => m.id === 92)?.result).toEqual({ answers: {} });
    notify("turn/completed", { turn: { id: "turn_1", status: "completed" } });
    await turn;
  });

  it.each([
    { decision: "allow", boolean: false },
    { decision: "deny", boolean: false },
    { decision: "allow", boolean: true },
    { decision: "deny", boolean: true },
  ] as const)(
    "shows MCP confirmation in Full Access and sends $decision, boolean=$boolean",
    async ({ decision, boolean }) => {
      const { events, turn } = await startTurn("codex-live", {
        runtimeMode: "full-access",
      });
      onLine!(
        JSON.stringify({
          id: 91,
          method: "mcpServer/elicitation/request",
          params: {
            serverName: "example",
            mode: "form",
            message: "Read this source?",
            requestedSchema: boolean
              ? {
                  type: "object",
                  properties: { approved: { type: "boolean" } },
                  required: ["approved"],
                }
              : { type: "object", properties: {} },
          },
        }),
      );
      await waitFor(
        () => events.some((e) => e.type === "approval.requested"),
        "MCP approval UI",
      );
      expect(parse().some((m) => m.id === 91)).toBe(false);
      const approval = events.find((e) => e.type === "approval.requested")!;
      respondCodexApproval("codex-live", approval.requestId, decision);
      await waitFor(() => parse().some((m) => m.id === 91), "MCP response");
      expect(parse().find((m) => m.id === 91)?.result).toEqual({
        action: decision === "allow" ? "accept" : "decline",
        content:
          decision === "allow" ? (boolean ? { approved: true } : {}) : null,
        _meta: null,
      });
      notify("turn/completed", { turn: { id: "turn_1", status: "completed" } });
      await turn;
    },
  );

  it.each([false, true, undefined])(
    "honors isBlocking=%s without relying on deprecated autoResolutionMs",
    async (isBlocking) => {
      const { events, turn } = await startTurn("codex-live");
      vi.useFakeTimers();
      onLine!(
        JSON.stringify({
          id: 91,
          method: "item/tool/requestUserInput",
          params: {
            isBlocking,
            autoResolutionMs: 1,
            questions: [
              { id: "q", question: "Choose a source", options: null },
            ],
          },
        }),
      );
      const question = events.find((event) => event.type === "question.asked")!;
      expect(question.autoResolveAt).toBe(
        isBlocking === false ? Date.now() + 120_000 : undefined,
      );
      await vi.advanceTimersByTimeAsync(119_999);
      expect(parse().some((m) => m.id === 91)).toBe(false);
      await vi.advanceTimersByTimeAsync(1);
      if (isBlocking === false) {
        expect(parse().find((m) => m.id === 91)?.result).toEqual({
          answers: {},
        });
        expect(events).toContainEqual({
          type: "question.resolved",
          requestId: question.requestId,
          decision: "skipped",
        });
      } else {
        expect(parse().some((m) => m.id === 91)).toBe(false);
      }
      notify("turn/completed", { turn: { id: "turn_1", status: "completed" } });
      await turn;
    },
  );

  it("keeps an optional question open after interaction and preserves its answer", async () => {
    const { events, turn } = await startTurn("codex-live");
    vi.useFakeTimers();
    onLine!(
      JSON.stringify({
        id: 91,
        method: "item/tool/requestUserInput",
        params: {
          isBlocking: false,
          questions: [{ id: "q", question: "Choose a source", options: null }],
        },
      }),
    );
    const question = events.find((event) => event.type === "question.asked")!;
    await vi.advanceTimersByTimeAsync(60_000);
    keepCodexQuestionOpen("codex-live", question.requestId);
    await vi.advanceTimersByTimeAsync(240_000);
    expect(parse().some((m) => m.id === 91)).toBe(false);
    const state = events.reduce(
      applyHarnessEvent,
      newSession("codex", "/repo", "codex:gpt-5.4", "supervised"),
    );
    expect(state.pendingQuestion?.autoResolveAt).toBeUndefined();
    respondCodexQuestion("codex-live", question.requestId, {
      kind: "answered",
      answers: {},
      custom: { q: "chosen source" },
    });
    await vi.advanceTimersByTimeAsync(0);
    expect(parse().find((m) => m.id === 91)?.result).toEqual({
      answers: { q: { answers: ["chosen source"] } },
    });
    notify("turn/completed", { turn: { id: "turn_1", status: "completed" } });
    await turn;
  });

  it("starts each queued optional question's deadline when it is shown", async () => {
    const { events, turn } = await startTurn("codex-live");
    vi.useFakeTimers();
    for (const id of [91, 92])
      onLine!(
        JSON.stringify({
          id,
          method: "item/tool/requestUserInput",
          params: {
            isBlocking: false,
            questions: [{ id: "q", question: `Question ${id}`, options: null }],
          },
        }),
      );
    expect(
      events.filter((event) => event.type === "question.asked"),
    ).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(120_000);
    expect(parse().filter((m) => m.id === 91)).toHaveLength(1);
    expect(parse().some((m) => m.id === 92)).toBe(false);
    const questions = events.filter((event) => event.type === "question.asked");
    expect(questions).toHaveLength(2);
    expect(questions[1].autoResolveAt).toBe(Date.now() + 120_000);
    await vi.advanceTimersByTimeAsync(120_000);
    expect(parse().filter((m) => m.id === 92)).toHaveLength(1);
    expect(
      events.reduce(
        applyHarnessEvent,
        newSession("codex", "/repo", "codex:gpt-5.4", "supervised"),
      ).pendingQuestion,
    ).toBeUndefined();
    notify("turn/completed", { turn: { id: "turn_1", status: "completed" } });
    await turn;
  });

  it.each(["answer", "server", "stop", "complete"])(
    "clears optional question timers on %s without a late reply",
    async (action) => {
      const { events, turn } = await startTurn("codex-live");
      vi.useFakeTimers();
      onLine!(
        JSON.stringify({
          id: 91,
          method: "item/tool/requestUserInput",
          params: {
            isBlocking: false,
            questions: [
              { id: "q", question: "Choose a source", options: null },
            ],
          },
        }),
      );
      const question = events.find((event) => event.type === "question.asked")!;
      if (action === "answer")
        respondCodexQuestion("codex-live", question.requestId, {
          kind: "skipped",
        });
      if (action === "server")
        notify("serverRequest/resolved", { threadId: "thr_1", requestId: 91 });
      if (action === "stop") await stopCodexSession("codex-live");
      if (action === "complete")
        notify("turn/completed", {
          turn: { id: "turn_1", status: "completed" },
        });
      await vi.advanceTimersByTimeAsync(0);
      expect(vi.getTimerCount()).toBe(0);
      await vi.advanceTimersByTimeAsync(240_000);
      respondCodexQuestion("codex-live", question.requestId, {
        kind: "answered",
        answers: {},
        custom: { q: "too late" },
      });
      await vi.advanceTimersByTimeAsync(0);
      expect(parse().filter((m) => m.id === 91)).toHaveLength(
        action === "answer" ? 1 : 0,
      );
      expect(
        events.filter((event) => event.type === "question.resolved"),
      ).toHaveLength(1);
      notify("turn/completed", { turn: { id: "turn_1", status: "completed" } });
      await turn;
    },
  );

  it("reports unsupported MCP forms instead of returning an empty success", async () => {
    const { events, turn } = await startTurn("codex-live");
    onLine!(
      JSON.stringify({
        id: 90,
        method: "item/tool/requestUserInput",
        params: {
          questions: [{ id: "q", question: "Choose a name", options: null }],
        },
      }),
    );
    onLine!(
      JSON.stringify({
        id: 91,
        method: "mcpServer/elicitation/request",
        params: {
          mode: "form",
          requestedSchema: {
            type: "object",
            properties: { name: { type: "string" } },
            required: ["name"],
          },
        },
      }),
    );
    await waitFor(() => parse().some((m) => m.id === 91), "MCP cancel");
    expect(parse().find((m) => m.id === 91)?.result).toEqual({
      action: "cancel",
      content: null,
      _meta: null,
    });
    expect(events).toContainEqual({
      type: "status",
      text: expect.stringContaining("does not support yet"),
    });
    onLine!(
      JSON.stringify({ id: 92, method: "future/requestApproval", params: {} }),
    );
    await waitFor(() => parse().some((m) => m.id === 92), "protocol error");
    expect(parse().find((m) => m.id === 92)?.error).toMatchObject({
      code: -32601,
    });
    const session = events.reduce(applyHarnessEvent, {
      ...newSession("codex", "/repo", "codex:gpt-5.4", "supervised"),
      busy: true,
    });
    expect(session.busy).toBe(true);
    expect(session.pendingQuestion?.questions[0].id).toBe("q");
    respondCodexQuestion("codex-live", session.pendingQuestion!.requestId, {
      kind: "answered",
      answers: {},
      custom: { q: "chosen name" },
    });
    await waitFor(() => parse().some((m) => m.id === 90), "remaining answer");
    expect(parse().find((m) => m.id === 90)?.result).toEqual({
      answers: { q: { answers: ["chosen name"] } },
    });
    notify("turn/completed", { turn: { id: "turn_1", status: "completed" } });
    await turn;
  });

  it("clears server-resolved approvals without replying twice", async () => {
    const { events, turn } = await startTurn("codex-live");
    onLine!(
      JSON.stringify({
        id: 91,
        method: "item/commandExecution/requestApproval",
        params: { itemId: "cmd", command: "git status" },
      }),
    );
    const request = events.find((e) => e.type === "approval.requested")!;
    notify("serverRequest/resolved", { threadId: "unrelated", requestId: 91 });
    await Promise.resolve();
    expect(events.some((e) => e.type === "approval.resolved")).toBe(false);
    notify("serverRequest/resolved", { threadId: "thr_1", requestId: 91 });
    await waitFor(
      () => events.some((e) => e.type === "approval.resolved"),
      "approval cleanup",
    );
    expect(events).toContainEqual({
      type: "approval.resolved",
      requestId: request.requestId,
      decision: "cancelled",
    });
    respondCodexApproval("codex-live", request.requestId, "allow");
    expect(parse().some((m) => m.id === 91)).toBe(false);
    notify("turn/completed", { turn: { id: "turn_1", status: "completed" } });
    await turn;
  });

  it("applies a queued access change only when that turn starts", async () => {
    const { events, turn } = await startTurn("codex-live");
    const queued = sendCodexTurn({
      sessionId: "codex-live",
      cwd: "/repo",
      model: "codex:gpt-5.4",
      runtimeMode: "full-access",
      text: "Continue",
      onEvent: (event) => events.push(event),
    });
    await Promise.resolve();
    onLine!(
      JSON.stringify({
        id: 91,
        method: "item/commandExecution/requestApproval",
        params: { itemId: "cmd", command: "git status" },
      }),
    );
    await waitFor(
      () => events.some((event) => event.type === "approval.requested"),
      "current turn approval",
    );
    expect(parse().some((m) => m.id === 91)).toBe(false);
    const request = events.find(
      (event) => event.type === "approval.requested",
    )!;
    respondCodexApproval("codex-live", request.requestId, "deny");
    await waitFor(
      () => parse().some((m) => m.id === 91),
      "current turn decision",
    );
    expect(parse().find((m) => m.id === 91)?.result).toEqual({
      decision: "decline",
    });
    notify("turn/completed", { turn: { id: "turn_1", status: "completed" } });
    await turn;
    await waitFor(
      () => parse().filter((m) => m.method === "turn/start").length === 2,
      "queued turn",
    );
    const next = parse().filter((m) => m.method === "turn/start")[1];
    expect(next.params).toMatchObject({
      approvalPolicy: "on-request",
      approvalsReviewer: "user",
      sandboxPolicy: { type: "dangerFullAccess" },
    });
    reply(next.id as number, { turn: { id: "turn_2" } });
    notify("turn/started", { turn: { id: "turn_2" } });
    onLine!(
      JSON.stringify({
        id: 92,
        method: "item/commandExecution/requestApproval",
        params: { itemId: "cmd2", command: "git status" },
      }),
    );
    await waitFor(
      () => parse().some((m) => m.id === 92),
      "queued turn approval",
    );
    expect(parse().find((m) => m.id === 92)?.result).toEqual({
      decision: "accept",
    });
    notify("turn/completed", { turn: { id: "turn_2", status: "completed" } });
    await queued;
  });

  it("stays busy after an agent message until turn/completed", async () => {
    const { events, turn } = await startTurn("codex-live");
    let settled = false;
    void turn.then(() => {
      settled = true;
    });

    notify("item/completed", {
      item: {
        id: "msg_1",
        type: "agentMessage",
        text: "I'll inspect the changelog first.",
      },
    });

    vi.useFakeTimers();
    await vi.advanceTimersByTimeAsync(3_000);
    expect(settled).toBe(false);
    vi.useRealTimers();

    notify("item/started", {
      item: {
        id: "cmd_1",
        type: "commandExecution",
        command: "git log -1",
        status: "inProgress",
      },
    });
    expect(settled).toBe(false);
    expect(events.some((event) => event.type === "tool.started")).toBe(true);

    notify("turn/completed", {
      turn: { id: "turn_1", status: "completed" },
    });
    await turn;
    expect(settled).toBe(true);
  });

  it("keeps plan turns read-only without surfacing approval prompts", async () => {
    const { events, turn } = await startTurn("codex-live", {
      runtimeMode: "auto",
      intent: "plan",
    });
    const turnStart = parse().find(
      (message) => message.method === "turn/start",
    );
    expect(turnStart?.params).toMatchObject({
      approvalPolicy: "never",
      sandboxPolicy: { type: "readOnly" },
      collaborationMode: { mode: "plan" },
    });

    onLine!(
      JSON.stringify({
        id: 91,
        method: "item/commandExecution/requestApproval",
        params: { itemId: "cmd_1", command: "git status --short" },
      }),
    );
    await waitFor(
      () => parse().some((message) => message.id === 91),
      "silent plan denial",
    );

    expect(events.some((event) => event.type === "approval.requested")).toBe(
      false,
    );
    expect(parse().find((message) => message.id === 91)?.result).toEqual({
      decision: "decline",
    });

    notify("turn/completed", {
      turn: { id: "turn_1", status: "completed" },
    });
    await turn;
  });

  it("uses thread/compact/start and waits for its turn to complete", async () => {
    const { turn } = await startTurn("codex-live");
    notify("turn/completed", {
      turn: { id: "turn_1", status: "completed" },
    });
    await turn;
    sent.length = 0;

    const compact = compactCodexContext({
      sessionId: "codex-live",
      cwd: "/repo",
      model: "codex:gpt-5.4",
      runtimeMode: "supervised",
      onEvent: () => undefined,
    });
    await waitFor(
      () =>
        parse().some((message) => message.method === "thread/compact/start"),
      "thread/compact/start",
    );
    const request = parse().find(
      (message) => message.method === "thread/compact/start",
    )!;
    expect(request.params).toEqual({ threadId: "thr_1" });
    reply(request.id as number, {});

    let settled = false;
    void compact.then(() => {
      settled = true;
    });
    notify("turn/started", {
      turn: { id: "compact_1", status: "inProgress" },
    });
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(settled).toBe(false);

    notify("turn/completed", {
      turn: { id: "compact_1", status: "completed" },
    });
    await compact;
    expect(settled).toBe(true);
  });
});
