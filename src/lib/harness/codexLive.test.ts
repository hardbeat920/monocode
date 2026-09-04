import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const sent: string[] = [];
let onLine: ((line: string) => void) | undefined;

vi.mock("./child", () => ({
  resolveCodexBinary: async () => ({ path: "/fake/codex" }),
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
  compactCodexContext,
  sendCodexTurn,
  stopCodexSession,
  __codexTestReset,
} = await import("./codex");
import type { HarnessEvent } from "./types";
import type { RuntimeMode, TurnIntent } from "../session";

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
  } = {},
) {
  const events: HarnessEvent[] = [];
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
  await waitFor(
    () => parse().some((m) => m.method === "thread/start"),
    "thread/start",
  );
  reply(parse().find((m) => m.method === "thread/start")!.id as number, {
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
  });

  afterEach(async () => {
    vi.useRealTimers();
    await stopCodexSession("codex-live");
    __codexTestReset();
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
