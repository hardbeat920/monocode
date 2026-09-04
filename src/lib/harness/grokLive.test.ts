import { describe, expect, it, vi, beforeEach } from "vitest";

const sent: string[] = [];
let onLine: ((line: string) => void) | undefined;
let onExit: ((code: number | null) => void) | undefined;

vi.mock("./child", () => ({
  resolveGrokBinary: async () => ({ path: "/fake/grok" }),
  spawnChild: async () => undefined,
  killChild: async () => undefined,
  unwatchChild: () => undefined,
  watchChild: (
    _id: string,
    line: (l: string) => void,
    exit: (c: number | null) => void,
  ) => {
    onLine = line;
    onExit = exit;
  },
  writeChild: async (_id: string, line: string) => {
    sent.push(line);
  },
}));

const {
  compactGrokContext,
  sendGrokTurn,
  respondGrokApproval,
  stopGrokSession,
} = await import("./grok");
import type { HarnessEvent } from "./types";

function reply(id: number, result: unknown) {
  onLine!(JSON.stringify({ jsonrpc: "2.0", id, result }));
}
const parse = () => sent.map((s) => JSON.parse(s));
const waitFor = async (pred: () => boolean, label: string) => {
  for (let i = 0; i < 200; i++) {
    if (pred()) return;
    await new Promise((r) => setTimeout(r, 5));
  }
  throw new Error(
    `timed out waiting for ${label}; sent=${JSON.stringify(parse().map((m) => m.method ?? `reply:${m.id}`))}`,
  );
};

const initResult = {
  protocolVersion: 1,
  authMethods: [{ id: "cached_token" }],
  _meta: {
    defaultAuthMethodId: "cached_token",
    modelState: {
      currentModelId: "grok-4.6",
      availableModels: [
        {
          modelId: "grok-4.6",
          name: "Grok 4.6",
          _meta: { totalContextTokens: 500000 },
        },
      ],
    },
  },
};

async function handshake() {
  await waitFor(
    () => parse().some((m) => m.method === "initialize"),
    "initialize",
  );
  reply(parse().find((m) => m.method === "initialize")!.id, initResult);
  await waitFor(
    () => parse().some((m) => m.method === "authenticate"),
    "authenticate",
  );
  reply(parse().find((m) => m.method === "authenticate")!.id, {});
  await waitFor(
    () => parse().some((m) => m.method === "session/new"),
    "session/new",
  );
  reply(parse().find((m) => m.method === "session/new")!.id, {
    sessionId: "S1",
    models: { currentModelId: "grok-4.6" },
  });
}

describe("grok live turn sequence", () => {
  beforeEach(() => {
    sent.length = 0;
  });

  it("authenticates, selects the model, and prompts", async () => {
    const events: HarnessEvent[] = [];
    const turn = sendGrokTurn({
      sessionId: "t1",
      cwd: "/repo",
      model: "grok:grok-4.6",
      modelSettings: { effort: "high" },
      runtimeMode: "supervised",
      text: "hey",
      attachments: [
        {
          id: "image-1",
          name: "screenshot.png",
          mimeType: "image/png",
          kind: "image",
          size: 3,
          data: "YWJj",
        },
      ],
      onEvent: (e) => events.push(e),
    });
    await handshake();
    await waitFor(
      () => parse().some((m) => m.method === "session/set_mode"),
      "set_mode",
    );
    reply(parse().find((m) => m.method === "session/set_mode")!.id, {});
    await waitFor(
      () => parse().some((m) => m.method === "session/prompt"),
      "prompt",
    );
    const promptRequest = parse().find((m) => m.method === "session/prompt")!;
    expect(promptRequest.params.prompt).toEqual([
      { type: "text", text: "hey" },
      { type: "image", mimeType: "image/png", data: "YWJj" },
    ]);
    reply(promptRequest.id, {
      stopReason: "end_turn",
    });
    await turn;
    expect(events.some((e) => e.type === "session.providerBound")).toBe(true);
    expect(parse().some((m) => m.method === "authenticate")).toBe(true);
    await stopGrokSession("t1");
  });

  it("surfaces a supervised permission request instead of auto-approving", async () => {
    const events: HarnessEvent[] = [];
    const turn = sendGrokTurn({
      sessionId: "t2",
      cwd: "/repo",
      model: "grok:grok-4.6",
      runtimeMode: "supervised",
      text: "run git",
      attachments: [],
      onEvent: (e) => events.push(e),
    });
    await handshake();
    await waitFor(
      () => parse().some((m) => m.method === "session/prompt"),
      "prompt",
    );
    const promptId = parse().find((m) => m.method === "session/prompt")!.id;
    onLine!(
      JSON.stringify({
        jsonrpc: "2.0",
        id: 1,
        method: "session/request_permission",
        params: {
          sessionId: "S1",
          toolCall: {
            toolCallId: "call_a",
            title: "Execute `git status`",
            kind: "execute",
            rawInput: { variant: "Bash", command: "git status" },
          },
          options: [
            { optionId: "allow-once", name: "Allow once", kind: "allow_once" },
            { optionId: "reject-once", name: "Reject", kind: "reject_once" },
          ],
        },
      }),
    );
    await waitFor(
      () => events.some((e) => e.type === "approval.requested"),
      "approval.requested",
    );
    respondGrokApproval("t2", 1, "allow");
    await waitFor(
      () => parse().some((m) => m.id === 1 && m.result),
      "permission response",
    );
    const response = parse().find((m) => m.id === 1 && m.result);
    expect(response.result.outcome.optionId).toBe("allow-once");
    reply(promptId, { stopReason: "end_turn" });
    await turn;
    await stopGrokSession("t2");
  });

  it("routes a late exit to the current turn's listener", async () => {
    const turn1Events: HarnessEvent[] = [];
    const turn2Events: HarnessEvent[] = [];
    const base = {
      sessionId: "t3",
      cwd: "/repo",
      model: "grok:grok-4.6",
      runtimeMode: "supervised" as const,
      attachments: [],
    };

    const turn1 = sendGrokTurn({
      ...base,
      text: "hey",
      onEvent: (e) => turn1Events.push(e),
    });
    await handshake();
    await waitFor(
      () => parse().some((m) => m.method === "session/prompt"),
      "prompt t1",
    );
    reply(parse().find((m) => m.method === "session/prompt")!.id, {
      stopReason: "end_turn",
    });
    await turn1;

    sent.length = 0;
    const turn2 = sendGrokTurn({
      ...base,
      text: "again",
      onEvent: (e) => turn2Events.push(e),
    });
    await waitFor(
      () => parse().some((m) => m.method === "session/prompt"),
      "prompt t2",
    );
    onExit!(1);
    await turn2.catch(() => undefined);

    expect(turn2Events.some((e) => e.type === "session.ended")).toBe(true);
    expect(turn1Events.some((e) => e.type === "session.ended")).toBe(false);
    await stopGrokSession("t3");
  });

  it("compacts with Grok's ACP extension instead of a slash-command prompt", async () => {
    const turn = sendGrokTurn({
      sessionId: "t4",
      cwd: "/repo",
      model: "grok:grok-4.6",
      runtimeMode: "supervised",
      text: "hey",
      attachments: [],
      onEvent: () => undefined,
    });
    await handshake();
    await waitFor(
      () => parse().some((message) => message.method === "session/prompt"),
      "session/prompt",
    );
    const prompt = parse().find(
      (message) => message.method === "session/prompt",
    )!;
    reply(prompt.id, { stopReason: "end_turn" });
    await turn;
    sent.length = 0;

    const compact = compactGrokContext({
      sessionId: "t4",
      cwd: "/repo",
      model: "grok:grok-4.6",
      runtimeMode: "supervised",
      onEvent: () => undefined,
    });
    await waitFor(
      () =>
        parse().some(
          (message) => message.method === "_x.ai/compact_conversation",
        ),
      "compact_conversation",
    );
    const request = parse().find(
      (message) => message.method === "_x.ai/compact_conversation",
    )!;
    expect(request.params).toEqual({ sessionId: "S1" });
    expect(parse().some((message) => message.method === "session/prompt")).toBe(
      false,
    );
    reply(request.id, {});
    await compact;
    await stopGrokSession("t4");
  });
});
