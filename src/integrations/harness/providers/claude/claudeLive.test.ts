import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { applyHarnessEvent } from "../../core/apply";
import { newSession } from "../../../../features/sessions/model/session";
import {
  foldableWork,
  foldedBlocks,
  groupTurnItems,
  workSummaryLine,
} from "../../../../features/sessions/model/transcriptActivity";

const sent: string[] = [];
const spawned: string[][] = [];
let onLine: ((line: string) => void) | undefined;
let onExit: ((code?: number | null) => void) | undefined;
let onStderr: ((line: string) => void) | undefined;
const writeChild = vi.fn(async (_id: string, line: string) => {
  sent.push(line);
});

vi.mock("../../core/child", () => ({
  resolveClaudeBinary: async () => ({ path: "/fake/claude" }),
  spawnChild: async (_id: string, _path: string, args: string[]) => {
    spawned.push(args);
  },
  killChild: async () => undefined,
  unwatchChild: () => undefined,
  watchChild: (
    _id: string,
    line: (l: string) => void,
    exit: (code?: number | null) => void,
    stderr?: (l: string) => void,
  ) => {
    onLine = line;
    onExit = exit;
    onStderr = stderr;
  },
  writeChild,
}));

const {
  bindClaudeSession,
  cancelClaudeTurn,
  compactClaudeContext,
  respondClaudeApproval,
  respondClaudeQuestion,
  sendClaudeTurn,
  stopClaudeSession,
  __claudeTestReset,
} = await import("./claude");
import type { HarnessEvent } from "../../core/types";
import type { RuntimeMode, TurnIntent } from "../../../../features/sessions/model/session";

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
  options: {
    runtimeMode?: RuntimeMode;
    intent?: TurnIntent;
    providerAccountId?: string;
  } = {},
) {
  const events: HarnessEvent[] = [];
  const turn = sendClaudeTurn({
    sessionId,
    cwd: "/repo",
    model: "claude:claude-sonnet-5",
    modelSettings: {},
    runtimeMode: options.runtimeMode ?? "supervised",
    intent: options.intent,
    providerAccountId: options.providerAccountId,
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

/** What Claude streams when a finished task wakes it for another turn. */
function emitFollowUpTurn(text: string) {
  emit({ type: "system", subtype: "init", session_id: "sess_1" });
  emit({
    type: "stream_event",
    session_id: "sess_1",
    event: {
      type: "content_block_delta",
      index: 0,
      delta: { type: "text_delta", text },
    },
  });
  emit({
    type: "assistant",
    session_id: "sess_1",
    message: { content: [{ type: "text", text }] },
  });
  emit({ type: "result", subtype: "success", session_id: "sess_1" });
}

function emitBackgroundBash(taskId = "b1") {
  emit({
    type: "assistant",
    session_id: "sess_1",
    message: {
      content: [
        {
          type: "tool_use",
          id: "toolu_bash",
          name: "Bash",
          input: { command: "sleep 30 && echo done", run_in_background: true },
        },
      ],
    },
  });
  emit({
    type: "system",
    subtype: "background_tasks_changed",
    tasks: [
      {
        task_id: taskId,
        task_type: "local_bash",
        description: "Wait 30 seconds then print done",
      },
    ],
  });
  emit({
    type: "system",
    subtype: "task_started",
    task_id: taskId,
    tool_use_id: "toolu_bash",
    description: "Wait 30 seconds then print done",
    task_type: "local_bash",
  });
  emit({
    type: "user",
    session_id: "sess_1",
    message: {
      content: [
        {
          type: "tool_result",
          tool_use_id: "toolu_bash",
          content: `Command running in background with ID: ${taskId}`,
        },
      ],
    },
  });
  emit({
    type: "stream_event",
    session_id: "sess_1",
    event: {
      type: "content_block_delta",
      index: 0,
      delta: { type: "text_delta", text: "waiting" },
    },
  });
  emit({
    type: "assistant",
    session_id: "sess_1",
    message: { content: [{ type: "text", text: "waiting" }] },
  });
  emit({ type: "result", subtype: "success", session_id: "sess_1" });
}

function emitBashFinished(taskId = "b1") {
  emit({ type: "system", subtype: "background_tasks_changed", tasks: [] });
  emit({
    type: "system",
    subtype: "task_updated",
    task_id: taskId,
    patch: { status: "completed" },
  });
  emit({
    type: "system",
    subtype: "task_notification",
    task_id: taskId,
    tool_use_id: "toolu_bash",
    status: "completed",
    summary: 'Background command "sleep 30 && echo done" completed (exit code 0)',
  });
}

function backgroundUpdates(events: HarnessEvent[]): string[][] {
  return events.flatMap((event) =>
    event.type === "background.updated" ? [event.tasks] : [],
  );
}

beforeEach(() => {
  sent.length = 0;
  spawned.length = 0;
  onLine = undefined;
  onExit = undefined;
  onStderr = undefined;
  writeChild.mockClear();
  __claudeTestReset();
});

afterEach(async () => {
  await stopClaudeSession("s1");
  __claudeTestReset();
});

describe("claude streamed tool inputs", () => {
  it("replaces an empty Shell row with the complete assistant tool input", async () => {
    const { events, turn } = await startTurn("s1");
    emit({
      type: "stream_event",
      session_id: "sess_1",
      event: {
        type: "content_block_start",
        index: 0,
        content_block: {
          type: "tool_use",
          id: "toolu_shell",
          name: "Bash",
          input: {},
        },
      },
    });
    emit({
      type: "stream_event",
      session_id: "sess_1",
      event: {
        type: "content_block_delta",
        index: 0,
        delta: {
          type: "input_json_delta",
          partial_json: '{"command":"git status',
        },
      },
    });
    emit({
      type: "assistant",
      session_id: "sess_1",
      message: {
        content: [
          {
            type: "tool_use",
            id: "toolu_shell",
            name: "Bash",
            input: {
              command: "git status --short",
              description: "Check changes",
            },
          },
        ],
      },
    });
    emit({
      type: "user",
      session_id: "sess_1",
      message: {
        content: [
          { type: "tool_result", tool_use_id: "toolu_shell", content: "clean" },
        ],
      },
    });
    emit({ type: "result", subtype: "success", session_id: "sess_1" });
    await turn;

    expect(events).toContainEqual(
      expect.objectContaining({
        type: "tool.updated",
        callId: "toolu_shell",
        title: "git status --short",
        status: "pending",
      }),
    );
    const session = events.reduce(
      applyHarnessEvent,
      newSession("claude", "/repo"),
    );
    const tool = session.blocks.find(
      (block) => block.tool?.callId === "toolu_shell",
    );
    expect(tool?.text).toBe("git status --short");
    expect(tool?.tool?.status).toBe("completed");
  });
});

describe("claude assistant message boundaries", () => {
  it("keeps a follow-up paragraph separate and does not replay its snapshot", async () => {
    const { events, turn } = await startTurn("s1");
    const progress = "- update the notes and commit";
    const update =
      "Connect returned an empty file for one image on one post. The catch-up skips it and carries on, and I'll include it in the final tally.";
    for (const text of [progress, update]) {
      emit({
        type: "stream_event",
        session_id: "sess_1",
        event: {
          type: "content_block_delta",
          index: 0,
          delta: { type: "text_delta", text },
        },
      });
      emit({
        type: "assistant",
        session_id: "sess_1",
        message: { content: [{ type: "text", text }] },
      });
    }
    emit({ type: "result", subtype: "success", session_id: "sess_1" });
    await turn;

    const session = events.reduce(
      applyHarnessEvent,
      newSession("claude", "/repo"),
    );
    expect(session.blocks.map((block) => block.text)).toEqual([
      progress,
      update,
    ]);
    expect(events.filter((event) => event.type === "message.delta")).toEqual([
      { type: "message.delta", text: progress },
      { type: "message.delta", text: update },
    ]);
  });
});

describe("claude model switching", () => {
  it("restarts a named account with the new model while resuming the provider conversation", async () => {
    const first = await startTurn("s1", {
      providerAccountId: "account-work",
    });
    emit({ type: "result", subtype: "success", session_id: "sess_1" });
    await first.turn;

    const userCount = parse().filter(
      (message) => message.type === "user",
    ).length;
    const second = sendClaudeTurn({
      sessionId: "s1",
      cwd: "/repo",
      model: "claude:opus-5",
      modelSettings: {},
      runtimeMode: "supervised",
      providerAccountId: "account-work",
      text: "what did I ask before?",
      attachments: [],
      onEvent: () => undefined,
    });

    await waitFor(() => spawned.length === 2, "replacement Claude process");
    expect(spawned[1]).toEqual(
      expect.arrayContaining([
        "--model",
        "claude-opus-5",
        "--resume",
        "sess_1",
      ]),
    );
    expect(spawned[1]).not.toContain("--session-id");

    emit({ type: "system", subtype: "init", session_id: "sess_1" });
    await waitFor(
      () =>
        parse().filter((message) => message.type === "user").length > userCount,
      "follow-up prompt",
    );
    emit({ type: "result", subtype: "success", session_id: "sess_1" });
    await second;
  });
});

describe("claude legacy account resume", () => {
  it("resumes a legacy thread when the missing account resolves to default", async () => {
    bindClaudeSession("s1", "legacy-session", "/repo");
    const { turn } = await startTurn("s1", {
      providerAccountId: "default",
    });
    expect(spawned[0]).toEqual(
      expect.arrayContaining(["--resume", "legacy-session"]),
    );
    expect(spawned[0]).not.toContain("--session-id");
    emit({ type: "result", subtype: "success", session_id: "legacy-session" });
    await turn;
  });

  it("does not resume a legacy default thread under a named account", async () => {
    bindClaudeSession("s1", "legacy-session", "/repo");
    const { turn } = await startTurn("s1", {
      providerAccountId: "account-work",
    });
    expect(spawned[0]).not.toContain("--resume");
    expect(spawned[0]).toContain("--session-id");
    emit({ type: "result", subtype: "success", session_id: "sess_1" });
    await turn;
  });
});

describe("claude poisoned resume", () => {
  it("starts a fresh conversation when Claude no longer has the resumed session", async () => {
    bindClaudeSession("s1", "dead-session", "/repo");
    const events: HarnessEvent[] = [];
    const turn = sendClaudeTurn({
      sessionId: "s1",
      cwd: "/repo",
      model: "claude:claude-sonnet-5",
      modelSettings: {},
      runtimeMode: "supervised",
      text: "explore the codebase",
      attachments: [],
      onEvent: (event) => events.push(event),
    });

    await waitFor(() => spawned.length === 1, "resume attempt");
    expect(spawned[0]).toEqual(
      expect.arrayContaining(["--resume", "dead-session"]),
    );

    // Claude rejects the unknown id on stderr and exits immediately.
    onStderr!("No conversation found with session ID: dead-session");
    onExit!(1);

    await waitFor(() => spawned.length === 2, "respawn without resume");
    expect(spawned[1]).not.toContain("--resume");
    expect(spawned[1]).toContain("--session-id");

    await waitFor(
      () =>
        parse().filter((m) => {
          const request = m.request as Record<string, unknown> | undefined;
          return request?.subtype === "initialize";
        }).length === 2,
      "second initialize",
    );
    emit({ type: "system", subtype: "init", session_id: "sess_1" });
    emit({
      type: "control_response",
      response: { subtype: "success", request_id: "monocode_1" },
    });
    await waitFor(() => parse().some((m) => m.type === "user"), "user prompt");
    emit({ type: "result", subtype: "success", session_id: "sess_1" });
    await turn;
  });
});

describe("claude failed startup", () => {
  it("fails the turn instead of handing back a session that never initialized", async () => {
    const events: HarnessEvent[] = [];
    const turn = sendClaudeTurn({
      sessionId: "s1",
      cwd: "/repo",
      model: "claude:claude-sonnet-5",
      modelSettings: {},
      runtimeMode: "supervised",
      text: "explore the codebase",
      attachments: [],
      onEvent: (event) => events.push(event),
    });

    await waitFor(() => spawned.length === 1, "spawn");
    // A startup failure with nothing recognisable on stderr: bad auth, a
    // crash, a flag the CLI rejects. The child simply never initializes.
    onStderr!("something the CLI has never said before");
    onExit!(1);

    await expect(turn).rejects.toThrow();
    // Returning a live handle here is what produced "Harness process is not
    // running" on the next write, so the session must never look started.
    expect(events.some((event) => event.type === "session.started")).toBe(
      false,
    );
    expect(spawned).toHaveLength(1);
  });
});

describe("claude subagents", () => {
  it.each(["allow", "deny"] as const)(
    "routes a child permission decision: %s",
    async (decision) => {
      const { events, turn } = await startTurn("s1");
      emit({
        type: "control_request",
        request_id: "child_permission",
        session_id: "sess_child",
        parent_tool_use_id: "toolu_agent",
        request: {
          subtype: "can_use_tool",
          tool_name: "Read",
          tool_use_id: "child_read",
          input: { file_path: "/home/user/.gitconfig" },
        },
      });
      const approval = events.find(
        (event) => event.type === "approval.requested",
      )!;
      expect(approval).toMatchObject({ callId: "child_read" });
      respondClaudeApproval("s1", approval.requestId, decision);
      await waitFor(
        () =>
          parse().some(
            (message) =>
              (message.response as Record<string, unknown>)?.request_id ===
              "child_permission",
          ),
        "child decision",
      );
      expect(
        parse().find(
          (message) =>
            (message.response as Record<string, unknown>)?.request_id ===
            "child_permission",
        ),
      ).toMatchObject({
        type: "control_response",
        response: { response: { behavior: decision } },
      });
      expect(
        events.filter((event) => event.type === "session.providerBound").at(-1),
      ).toMatchObject({ providerSessionId: "sess_1" });
      emit({ type: "result", subtype: "success", session_id: "sess_1" });
      await turn;
    },
  );

  it("keeps simultaneous child questions reachable in the single-question UI", async () => {
    const { events, turn } = await startTurn("s1");
    for (const id of ["child_a", "child_b"]) {
      emit({
        type: "control_request",
        request_id: id,
        parent_tool_use_id: `agent_${id}`,
        request: {
          subtype: "can_use_tool",
          tool_name: "AskUserQuestion",
          input: {
            questions: [
              {
                question: `Question from ${id}`,
                options: [{ label: "Proceed" }],
              },
            ],
          },
        },
      });
    }
    expect(
      events.filter((event) => event.type === "question.asked"),
    ).toHaveLength(1);
    for (const id of ["child_a", "child_b"]) {
      const session = events.reduce(
        applyHarnessEvent,
        newSession("claude", "/repo"),
      );
      const request = session.pendingQuestion!;
      expect(request.questions[0].prompt).toBe(`Question from ${id}`);
      respondClaudeQuestion(
        "s1",
        request.requestId,
        id === "child_a"
          ? {
              kind: "answered",
              answers: {
                [request.questions[0].id]: [request.questions[0].options[0].id],
              },
            }
          : { kind: "skipped" },
      );
      await waitFor(
        () =>
          parse().some(
            (message) =>
              (message.response as Record<string, unknown>)?.request_id === id,
          ),
        "question response",
      );
      expect(
        parse().find(
          (message) =>
            (message.response as Record<string, unknown>)?.request_id === id,
        ),
      ).toMatchObject({
        response: {
          response: { behavior: id === "child_a" ? "allow" : "deny" },
        },
      });
    }
    expect(
      events.reduce(applyHarnessEvent, newSession("claude", "/repo"))
        .pendingQuestion,
    ).toBeUndefined();
    emit({ type: "result", subtype: "success", session_id: "sess_1" });
    await turn;
  });

  it.each(["child_a", "child_b"])(
    "preserves the remaining question when %s is cancelled by the server",
    async (cancelled) => {
      const { events, turn } = await startTurn("s1");
      for (const id of ["child_a", "child_b"]) {
        emit({
          type: "control_request",
          request_id: id,
          parent_tool_use_id: `agent_${id}`,
          request: {
            subtype: "can_use_tool",
            tool_name: "AskUserQuestion",
            input: {
              questions: [{ question: id, options: [{ label: "Proceed" }] }],
            },
          },
        });
      }
      emit({ type: "control_cancel_request", request_id: cancelled });
      await waitFor(
        () => events.some((event) => event.type === "question.resolved"),
        "cancelled question",
      );
      const session = events.reduce(
        applyHarnessEvent,
        newSession("claude", "/repo"),
      );
      const remaining = cancelled === "child_a" ? "child_b" : "child_a";
      expect(session.pendingQuestion?.questions[0].prompt).toBe(remaining);
      respondClaudeQuestion("s1", session.pendingQuestion!.requestId, {
        kind: "skipped",
      });
      await waitFor(
        () =>
          parse().some(
            (message) =>
              (message.response as Record<string, unknown>)?.request_id ===
              remaining,
          ),
        "remaining question response",
      );
      expect(
        parse().some(
          (message) =>
            (message.response as Record<string, unknown>)?.request_id ===
            cancelled,
        ),
      ).toBe(false);
      emit({ type: "result", subtype: "success", session_id: "sess_1" });
      await turn;
    },
  );

  it("fails the active turn if a child permission reply cannot be delivered", async () => {
    const { events, turn } = await startTurn("s1");
    emit({
      type: "control_request",
      request_id: "child_permission",
      parent_tool_use_id: "toolu_agent",
      request: {
        subtype: "can_use_tool",
        tool_name: "Read",
        input: { file_path: "/home/user/.gitconfig" },
      },
    });
    const approval = events.find(
      (event) => event.type === "approval.requested",
    )!;
    let outcome: unknown;
    void turn.catch((error) => {
      outcome = error;
    });
    writeChild.mockRejectedValueOnce(new Error("Broken pipe"));
    respondClaudeApproval("s1", approval.requestId, "allow");
    await waitFor(() => outcome instanceof Error, "failed permission delivery");
    expect(outcome).toMatchObject({ message: "Broken pipe" });
    expect(events).toContainEqual({
      type: "session.error",
      message: "Broken pipe",
    });
  });

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
    // The notification wakes Claude for a follow-up turn; that turn's result
    // is what ends the MonoCode turn.
    await new Promise((r) => setTimeout(r, 30));
    expect(settled).toBe(false);
    emitFollowUpTurn("The explorer found the tokens.");
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

  it("mirrors a subagent's tools, thinking and prose onto its own row", async () => {
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
              description: "Correctness review",
              subagent_type: "explore",
            },
          },
        ],
      },
    });
    emit({
      type: "assistant",
      parent_tool_use_id: "toolu_agent",
      message: {
        id: "msg_sub_1",
        model: "claude-haiku-4-5",
        content: [
          { type: "thinking", thinking: "Start with the reducer." },
          { type: "text", text: "I will grep for tokens" },
          {
            type: "tool_use",
            id: "toolu_sub_read",
            name: "Read",
            input: { file_path: "/repo/src/App.tsx" },
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
            tool_use_id: "toolu_sub_read",
            content: "export function App() {}",
          },
        ],
      },
    });
    emit({ type: "result", subtype: "success", session_id: "sess_1" });
    await turn;

    expect(
      events
        .reduce(applyHarnessEvent, newSession("claude", "/repo"))
        .blocks.find((block) => block.tool?.callId === "toolu_agent")?.agentRun
        ?.model,
    ).toBe("claude-haiku-4-5");
    const steps = events.filter((event) => event.type === "agent.step");
    expect(steps.every((step) => step.callId === "toolu_agent")).toBe(true);
    expect(
      steps.map((step) => [step.stepId, step.kind, step.text, step.status]),
    ).toEqual([
      ["msg_sub_1:thinking", "reasoning", "Start with the reducer.", undefined],
      ["msg_sub_1:text", "message", "I will grep for tokens", undefined],
      ["toolu_sub_read", "tool", "Read /repo/src/App.tsx", "in_progress"],
      ["toolu_sub_read", "tool", "", "completed"],
    ]);
  });

  it("does not mirror a subagent result onto the parent tool row", async () => {
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
            input: { description: "Correctness review" },
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
            tool_use_id: "toolu_sub_read",
            content: "export function App() {}",
          },
        ],
      },
    });
    emit({ type: "result", subtype: "success", session_id: "sess_1" });
    await turn;

    // The parent stays in flight: only the subagent's own row settles.
    expect(
      events.some(
        (event) =>
          event.type === "tool.updated" &&
          event.callId === "toolu_agent" &&
          event.status === "completed",
      ),
    ).toBe(false);
  });

  it("routes an unexpected provider exit to the turn that is actually running", async () => {
    const first = await startTurn("s1");
    emit({ type: "result", subtype: "success", session_id: "sess_1" });
    await first.turn;

    const secondEvents: HarnessEvent[] = [];
    const userMessages = parse().filter(
      (message) => message.type === "user",
    ).length;
    const second = sendClaudeTurn({
      sessionId: "s1",
      cwd: "/repo",
      model: "claude:claude-sonnet-5",
      runtimeMode: "supervised",
      text: "try again",
      attachments: [],
      onEvent: (event) => secondEvents.push(event),
    });
    await waitFor(
      () =>
        parse().filter((message) => message.type === "user").length >
        userMessages,
      "second user prompt",
    );

    onExit?.(1);
    await expect(second).rejects.toThrow("Claude Code exited");
    expect(first.events.some((event) => event.type === "session.ended")).toBe(
      false,
    );
    expect(secondEvents).toContainEqual({ type: "session.ended", code: 1 });
    expect(secondEvents).toContainEqual({
      type: "session.error",
      message: "Claude Code exited",
    });
  });
});

describe("claude background tasks", () => {
  it("keeps the turn working through a background command and Claude's follow-up", async () => {
    const { events, turn } = await startTurn("s1");
    let settled = false;
    void turn.then(() => {
      settled = true;
    });

    emitBackgroundBash();
    await new Promise((r) => setTimeout(r, 30));
    expect(settled).toBe(false);
    expect(events.some((event) => event.type === "message.completed")).toBe(
      false,
    );
    expect(backgroundUpdates(events)).toEqual([
      ["Wait 30 seconds then print done"],
    ]);

    emitBashFinished();
    await new Promise((r) => setTimeout(r, 30));
    expect(settled).toBe(false);

    emitFollowUpTurn("It finished and printed done.");
    await turn;
    expect(backgroundUpdates(events)).toEqual([
      ["Wait 30 seconds then print done"],
      [],
    ]);
    // The command waited on sits under the message Claude left off with as
    // a row of its own, and the reply is a new message after it, once.
    const session = events.reduce(
      applyHarnessEvent,
      newSession("claude", "/repo"),
    );
    const turn1 = session.blocks.slice(1);
    const background = turn1.find((block) => block.tool?.background);
    expect(background?.tool).toMatchObject({
      callId: "background:b1",
      status: "completed",
      detail:
        'Background command "sleep 30 && echo done" completed (exit code 0)',
    });
    expect(background?.text).toContain("sleep 30");
    expect(
      turn1
        .filter((block) => block.role === "assistant" || block.tool?.background)
        .map((block) => (block.tool?.background ? "[background]" : block.text)),
    ).toEqual(["waiting", "[background]", "It finished and printed done."]);
    const items = groupTurnItems(turn1, { settled: true });
    const fold = foldableWork(items);
    const answer = items.at(-1);
    expect(answer?.type === "block" && answer.block.text).toBe(
      "It finished and printed done.",
    );
    // What Claude yielded with is its answer; the follow-up does not fold it.
    expect(
      (fold ? foldedBlocks(items, fold) : []).map((block) => block.text),
    ).not.toContain("waiting");
  });

  it("shows the waited-on command as a live row under Claude's last message", async () => {
    const { events } = await startTurn("s1");
    emitBackgroundBash();
    await new Promise((r) => setTimeout(r, 10));

    const session = events.reduce(
      applyHarnessEvent,
      newSession("claude", "/repo"),
    );
    const last = session.blocks.at(-1);
    expect(session.blocks.at(-2)?.text).toBe("waiting");
    expect(last?.tool).toMatchObject({
      background: true,
      status: "in_progress",
      kind: "execute",
    });
    const items = groupTurnItems(session.blocks.slice(1));
    const group = items.at(-1);
    expect(group?.type === "activity" && workSummaryLine(group.blocks, true)).toBe(
      "Running in background",
    );
  });

  it("lets the turn go if a finished task never wakes Claude", async () => {
    const { turn } = await startTurn("s1");
    let settled = false;
    void turn.then(() => {
      settled = true;
    });
    emitBackgroundBash();

    vi.useFakeTimers();
    try {
      emitBashFinished();
      await vi.advanceTimersByTimeAsync(10_000);
      expect(settled).toBe(false);
      await vi.advanceTimersByTimeAsync(10_000);
      expect(settled).toBe(true);
    } finally {
      vi.useRealTimers();
    }
  });

  it("stops background commands when the turn is stopped", async () => {
    const { events, turn } = await startTurn("s1");
    emitBackgroundBash("b7");

    await cancelClaudeTurn("s1");
    await turn;
    const requests = parse().flatMap((m) => {
      const request = m.request as Record<string, unknown> | undefined;
      return request ? [request] : [];
    });
    expect(requests).toContainEqual({ subtype: "stop_task", task_id: "b7" });
    expect(requests.at(-1)).toEqual({ subtype: "interrupt" });
    expect(events.some((event) => event.type === "message.completed")).toBe(
      true,
    );
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
