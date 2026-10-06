import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { HarnessEvent } from "../../core/types";

const native = vi.hoisted(() => ({
  run: vi.fn(),
  cancel: vi.fn(),
  steer: vi.fn(),
}));
const sent: Record<string, unknown>[] = [];
const spawned: string[][] = [];
let onLine: (line: string) => void;
vi.mock("../../core/child", () => ({
  resolveClaudeBinary: async () => ({ path: "/fake/claude" }),
  spawnChild: async (_id: string, _path: string, args: string[]) => {
    spawned.push(args);
  },
  killChild: async () => undefined,
  unwatchChild: () => undefined,
  watchChild: (_id: string, line: (line: string) => void) => {
    onLine = line;
  },
  writeChild: async (_id: string, line: string) => {
    sent.push(JSON.parse(line));
  },
}));
vi.mock("./claudeComputerUse", async (original) => ({
  ...(await original<typeof import("./claudeComputerUse")>()),
  computerUseConfig: async () => ({ mcpServers: {} }),
  ComputerUseRun: class {
    run = native.run;
    cancel = native.cancel;
    steer = native.steer;
  },
}));

const {
  sendClaudeTurn,
  respondClaudeApproval,
  cancelClaudeTurn,
  __claudeTestReset,
} = await import("./claude");

const emit = (rec: Record<string, unknown>) => onLine(JSON.stringify(rec));
async function until(predicate: () => boolean) {
  for (let i = 0; i < 200; i++) {
    if (predicate()) return;
    await new Promise((resolve) => setTimeout(resolve, 5));
  }
  throw new Error("Test event did not arrive");
}

beforeEach(() => {
  __claudeTestReset();
  sent.length = 0;
  spawned.length = 0;
  native.run.mockReset().mockResolvedValue(undefined);
  native.cancel.mockReset().mockResolvedValue(undefined);
  native.steer.mockReset().mockResolvedValue(undefined);
});
afterEach(() => __claudeTestReset());

function send(events: HarnessEvent[], intent?: "plan") {
  return sendClaudeTurn({
    sessionId: "thread",
    cwd: "/repo",
    model: "claude:sonnet",
    runtimeMode: "full-access",
    providerAccountId: "account",
    modelSettings: { effort: "high" },
    intent,
    text: "Use native computer use for Calculator",
    onEvent: (event) => events.push(event),
  });
}
async function initialize() {
  await until(() =>
    sent.some(
      (row) =>
        (row.request as Record<string, unknown> | undefined)?.subtype ===
        "initialize",
    ),
  );
  emit({ type: "system", subtype: "init", session_id: "provider-session" });
  await until(() => sent.some((row) => row.type === "user"));
}
function handoff(failed = false) {
  emit({
    type: "assistant",
    message: {
      content: [
        {
          type: "tool_use",
          id: "handoff",
          name: "mcp__monocode_computer_use__start",
          input: { task: "Request Calculator access" },
        },
      ],
    },
  });
  emit({
    type: "user",
    message: {
      content: [
        {
          type: "tool_result",
          tool_use_id: "handoff",
          is_error: failed,
          content: failed ? "Denied" : "Handoff requested",
        },
      ],
    },
  });
  emit({ type: "result", subtype: "success" });
}

it("hands off a successful routing tool and returns to stream-json with the same account and session", async () => {
  const events: HarnessEvent[] = [];
  const turn = send(events);
  await initialize();
  handoff();
  await turn;
  expect(native.run).toHaveBeenCalledWith(
    expect.objectContaining({
      providerSessionId: "provider-session",
      accountId: "account",
      cwd: "/repo",
    }),
  );
  expect(native.run.mock.calls[0][0].args).toContain("bypassPermissions");
  expect(native.run.mock.calls[0][0].args).toContain("Bash");
  sent.length = 0;
  const next = send(events);
  await initialize();
  expect(spawned[1]).toEqual(
    expect.arrayContaining(["--resume", "provider-session"]),
  );
  emit({ type: "result", subtype: "success" });
  await next;
});

it("shows a human approval card for native apps even when ordinary tool permissions are bypassed", async () => {
  const events: HarnessEvent[] = [];
  native.run.mockImplementation(async (input) => {
    expect(
      await input.approve("Allow Calculator?", "Calculator: session access"),
    ).toBe("deny");
  });
  const turn = send(events);
  await initialize();
  handoff();
  await until(() =>
    events.some((event) => event.type === "approval.requested"),
  );
  const approval = events.find(
    (event) => event.type === "approval.requested",
  ) as Extract<HarnessEvent, { type: "approval.requested" }>;
  respondClaudeApproval("thread", approval.requestId, "deny");
  await turn;
  expect(events).toContainEqual({
    type: "approval.resolved",
    requestId: approval.requestId,
    decision: "deny",
  });
});

it("does not hand off a failed routing tool or a planning turn", async () => {
  const turn = send([]);
  await initialize();
  handoff(true);
  await turn;
  expect(native.run).not.toHaveBeenCalled();
  __claudeTestReset();
  sent.length = 0;
  const plan = send([], "plan");
  await initialize();
  handoff();
  await plan;
  expect(native.run).not.toHaveBeenCalled();
});

it("cancels a native approval and closes the interactive run", async () => {
  const events: HarnessEvent[] = [];
  native.run.mockImplementation(async (input) => {
    await input.approve("Allow Calculator?", "Calculator");
  });
  const turn = send(events);
  await initialize();
  handoff();
  await until(() =>
    events.some((event) => event.type === "approval.requested"),
  );
  await cancelClaudeTurn("thread");
  await turn;
  expect(native.cancel).toHaveBeenCalledOnce();
});
