import { beforeEach, describe, expect, it, vi } from "vitest";
import type { HarnessEvent, SendTurnInput } from "../../core/types";

const sent: string[] = [];
let cancelWrite: Promise<void> | undefined;
let onLine: ((line: string) => void) | undefined;
let onExit: ((code: number | null) => void) | undefined;
let onStderr: ((line: string) => void) | undefined;
/** Records process launch arguments without starting a child. */
const spawn = vi.fn(async (..._args: unknown[]) => undefined);
/** Records cleanup attempts without terminating a process. */
const kill = vi.fn(async (..._args: unknown[]) => undefined);

vi.mock("../../core/child", () => ({
  /** Supplies a deterministic executable path without probing the host. */
  resolveCopilotBinary: async () => ({ path: "/fake/copilot" }),
  /** Forwards launch arguments to the spawn spy. */
  spawnChild: (...args: Parameters<typeof spawn>) => spawn(...args),
  /** Forwards termination requests to the cleanup spy. */
  killChild: (...args: unknown[]) => kill(...args),
  /** Keeps captured callbacks available for assertions about late transport events. */
  unwatchChild: () => undefined,
  /** Captures transport callbacks so tests can deliver replies, exits, and stderr directly. */
  watchChild: (
    _id: string,
    line: (value: string) => void,
    exit: (code: number | null) => void,
    stderr: (value: string) => void,
  ) => {
    onLine = line;
    onExit = exit;
    onStderr = stderr;
  },
  /** Records outgoing RPCs and optionally delays cancellation writes to exercise ordering races. */
  writeChild: async (_id: string, line: string) => {
    sent.push(line);
    if (JSON.parse(line).method === "session/cancel") await cancelWrite;
  },
}));

const {
  bindCopilotSession,
  cancelCopilotTurn,
  forgetCopilotSession,
  respondCopilotApproval,
  sendCopilotTurn,
  steerCopilotTurn,
  stopCopilotSession,
} = await import("./copilot");

/** Decodes all outgoing transport lines for RPC assertions. */
const parse = () => sent.map((line) => JSON.parse(line));

/** Delivers a successful JSON-RPC response through the captured child listener. */
function reply(id: number, result: unknown) {
  onLine!(JSON.stringify({ jsonrpc: "2.0", id, result }));
}

/** Waits for an outgoing RPC method and returns its first matching message. */
async function request(method: string) {
  await vi.waitFor(() => expect(parse().some((message) => message.method === method)).toBe(true));
  return parse().find((message) => message.method === method)!;
}

/** Starts a supervised turn with deterministic defaults and caller-provided overrides. */
function start(sessionId: string, events: HarnessEvent[], changes: Partial<SendTurnInput> = {}) {
  return sendCopilotTurn({
    sessionId,
    cwd: "/repo",
    model: "copilot:auto",
    runtimeMode: "supervised",
    text: "create file",
    attachments: [],
    /** Collects emitted events for assertions about the turn lifecycle. */
    onEvent: (event) => events.push(event),
    ...changes,
  });
}

/** Verifies initialization and session creation, then replies with the requested current model. */
async function ready(currentModelId = "claude-sonnet-5") {
  const init = await request("initialize");
  expect(init.params).toEqual({
    protocolVersion: 1,
    clientCapabilities: { fs: { readTextFile: false, writeTextFile: false }, terminal: false },
    clientInfo: { name: "monocode", version: "0.1.0" },
  });
  reply(init.id, { protocolVersion: 1 });
  const created = await request("session/new");
  expect(created.params).toEqual({ cwd: "/repo", mcpServers: [] });
  reply(created.id, { sessionId: "copilot-session-1", models: { currentModelId } });
}

/** Verifies and acknowledges model selection followed by the requested ACP session mode. */
async function selectModelAndMode(
  sessionId: string,
  modelId: string,
  sessionMode = "agent",
) {
  const model = await request("session/set_model");
  expect(model.params).toEqual({ sessionId, modelId });
  reply(model.id, {});
  const mode = await request("session/set_mode");
  expect(mode.params.modeId).toBe(
    `https://agentclientprotocol.com/protocol/session-modes#${sessionMode}`,
  );
  reply(mode.id, {});
}

describe("Copilot live ACP sequence", () => {
  beforeEach(() => {
    sent.length = 0;
    cancelWrite = undefined;
    onLine = undefined;
    onExit = undefined;
    onStderr = undefined;
    spawn.mockClear();
    kill.mockClear();
  });

  it.each(
    (["success", "failure", "cancel"] as const).flatMap((outcome) =>
      (["old-first", "latest-first"] as const).map((order) => ({ outcome, order })),
    ),
  )("keeps the send lifecycle through rapid steering: $outcome, $order", async ({ outcome, order }) => {
    const events: HarnessEvent[] = [];
    const sessionId = `copilot-steer-${outcome}-${order}`;
    let settled = false;
    const turn = start(sessionId, events).then(
      () => { settled = true; },
      (error: unknown) => { settled = true; return error; },
    );
    await ready();
    await selectModelAndMode("copilot-session-1", "auto");
    const first = await request("session/prompt");
    /** Sends guidance to the active test session without starting a separate turn. */
    const steer = (text: string) => steerCopilotTurn({
      sessionId, cwd: "/repo", model: "copilot:auto", text,
    });
    try {
      await steer("");
      expect(parse().filter((message) => message.method === "session/prompt")).toHaveLength(1);
      await steer("first follow up");
      await steer("latest follow up");
      const prompts = parse().filter((message) => message.method === "session/prompt");
      expect(prompts).toHaveLength(3);
      expect(prompts[2].params.prompt).toEqual([{ type: "text", text: "latest follow up" }]);
      /** Settles superseded requests with cancellation and failure to test stale-response handling. */
      const settleOld = () => {
        reply(first.id, { stopReason: "cancelled" });
        onLine!(JSON.stringify({ jsonrpc: "2.0", id: prompts[1].id, error: { code: -32000, message: "superseded" } }));
      };
      if (order === "old-first") {
        settleOld();
        await new Promise((resolve) => setTimeout(resolve, 0));
        expect(settled).toBe(false);
        expect(events.some((event) => event.type === "message.completed" || event.type === "session.error")).toBe(false);
      }

      if (outcome === "success") {
        reply(prompts[2].id, { stopReason: "end_turn", usage: { inputTokens: 20, outputTokens: 5 } });
      } else if (outcome === "failure") {
        onLine!(JSON.stringify({ jsonrpc: "2.0", id: prompts[2].id, error: { code: -32000, message: "latest failed" } }));
      } else {
        await cancelCopilotTurn(sessionId);
      }
      await vi.waitFor(() => expect(settled).toBe(true));
      if (order === "latest-first") {
        const completedEvents = [...events];
        settleOld();
        await new Promise((resolve) => setTimeout(resolve, 0));
        expect(events).toEqual(completedEvents);
      }
      const result = await turn;
      expect(events.filter((event) => event.type === "message.completed")).toHaveLength(outcome === "success" ? 1 : 0);
      expect(events.filter((event) => event.type === "reasoning.completed")).toHaveLength(outcome === "success" ? 1 : 0);
      expect(events.filter((event) => event.type === "turn.metrics")).toHaveLength(outcome === "success" ? 1 : 0);
      expect(events.filter((event) => event.type === "session.error")).toEqual(outcome === "failure" ? [{ type: "session.error", message: "latest failed" }] : []);
      if (outcome === "failure") {
        expect(result).toEqual(new Error("latest failed"));
        expect(kill).toHaveBeenCalledWith(sessionId);
      } else {
        expect(result).toBeUndefined();
      }
      await expect(steer("after settlement")).rejects.toThrow("No active Copilot CLI turn");
    } finally {
      await cancelCopilotTurn(sessionId);
      await turn;
      await stopCopilotSession(sessionId);
    }
  });

  it("starts ACP, selects model and mode, streams text, tools and context, then reports usage", async () => {
    const events: HarnessEvent[] = [];
    const turn = start("copilot-new", events, {
      attachments: [{ id: "image-1", name: "screen.png", mimeType: "image/png", kind: "image", size: 4, data: "AAAA" }],
    });
    await ready();
    await selectModelAndMode("copilot-session-1", "auto");
    const prompt = await request("session/prompt");
    expect(spawn).toHaveBeenCalledWith("copilot-new", "/fake/copilot", ["--acp", "--stdio"], "/repo", undefined, "copilot");
    expect(prompt.params.prompt).toEqual([
      { type: "text", text: "create file" },
      { type: "image", mimeType: "image/png", data: "AAAA" },
    ]);
    /** Delivers a session update notification through the mock ACP transport. */
    const update = (value: unknown) => onLine!(JSON.stringify({
      jsonrpc: "2.0", method: "session/update", params: { sessionId: "copilot-session-1", update: value },
    }));
    update({ sessionUpdate: "usage_update", used: 16860, size: 272000 });
    update({ sessionUpdate: "tool_call", toolCallId: "call-file", title: "apply_patch", kind: "edit", status: "pending", rawInput: "*** Begin Patch\n*** Add File: hello.txt\n+hi\n*** End Patch\n" });
    update({ sessionUpdate: "tool_call_update", toolCallId: "call-file", status: "completed", content: [{ type: "diff", path: "/repo/hello.txt", oldText: "", newText: "hi\n" }] });
    update({ sessionUpdate: "agent_message_chunk", content: { type: "text", text: "DONE" } });
    reply(prompt.id, { stopReason: "end_turn", usage: { inputTokens: 100, outputTokens: 10, cachedReadTokens: 50, cachedWriteTokens: 0 } });
    await turn;
    expect(events).toContainEqual({ type: "session.providerBound", providerSessionId: "copilot-session-1" });
    expect(events).toContainEqual({ type: "session.started" });
    expect(events).toContainEqual(expect.objectContaining({ type: "context", used: 16860, window: 272000 }));
    expect(events).toContainEqual(expect.objectContaining({
      type: "tool.updated",
      callId: "call-file",
      preview: expect.objectContaining({ path: "/repo/hello.txt", additions: 1 }),
    }));
    expect(events).toContainEqual(expect.objectContaining({ type: "message.delta", text: "DONE" }));
    expect(events).toContainEqual({ type: "message.completed" });
    expect(events).toContainEqual({ type: "reasoning.completed" });
    expect(events).toContainEqual(expect.objectContaining({ type: "turn.metrics", inputTokens: 100, outputTokens: 10, cacheReadTokens: 50, cacheWriteTokens: 0 }));
    const metrics = events.find(
      (event): event is Extract<HarnessEvent, { type: "turn.metrics" }> =>
        event.type === "turn.metrics",
    );
    expect(metrics?.cacheHitPercent).toBeCloseTo(100 / 3);
    await stopCopilotSession("copilot-new");
  });

  it("uses advertised allow_once for a supervised edit permission", async () => {
    const events: HarnessEvent[] = [];
    const turn = start("copilot-permission", events);
    await ready();
    await selectModelAndMode("copilot-session-1", "auto");
    const prompt = await request("session/prompt");
    onLine!(JSON.stringify({ jsonrpc: "2.0", id: 42, method: "session/request_permission", params: {
      sessionId: "copilot-session-1",
      toolCall: { toolCallId: "call-edit", title: "Create file", kind: "edit", status: "pending", rawInput: { fileName: "/repo/hello.txt", diff: "diff --git a/hello.txt b/hello.txt" } },
      options: [
        { optionId: "allow_once", kind: "allow_once", name: "Allow once" },
        { optionId: "allow_always", kind: "allow_always", name: "Always allow" },
        { optionId: "reject_once", kind: "reject_once", name: "Deny" },
      ],
    } }));
    expect(events).toContainEqual(expect.objectContaining({ type: "approval.requested", requestId: 42, callId: "call-edit", kind: "edit" }));
    respondCopilotApproval("copilot-permission", 42, "allow");
    await vi.waitFor(() => expect(parse().some((message) => message.id === 42 && message.result)).toBe(true));
    expect(parse().find((message) => message.id === 42)?.result).toEqual({ outcome: { outcome: "selected", optionId: "allow_once" } });
    reply(prompt.id, { stopReason: "end_turn" });
    await turn;
    await stopCopilotSession("copilot-permission");
  });

  it("loads bound sessions without creating a new session", async () => {
    bindCopilotSession("copilot-load", "persisted-session", "/repo");
    const turn = start("copilot-load", [], { model: "copilot:claude-sonnet-5" });
    const init = await request("initialize");
    reply(init.id, { protocolVersion: 1 });
    const load = await request("session/load");
    expect(load.params).toEqual({ sessionId: "persisted-session", cwd: "/repo", mcpServers: [] });
    reply(load.id, { models: { currentModelId: "claude-sonnet-5" } });
    const mode = await request("session/set_mode");
    reply(mode.id, {});
    const prompt = await request("session/prompt");
    reply(prompt.id, { stopReason: "end_turn" });
    await turn;
    expect(parse().some((message) => message.method === "session/new" || message.method === "session/set_model")).toBe(false);
    await stopCopilotSession("copilot-load");
  });

  it("falls back to a new session after a failed load", async () => {
    bindCopilotSession("copilot-stale", "stale-session", "/repo");
    const events: HarnessEvent[] = [];
    const turn = start("copilot-stale", events);
    const init = await request("initialize");
    reply(init.id, { protocolVersion: 1 });
    const load = await request("session/load");
    onLine!(JSON.stringify({ jsonrpc: "2.0", id: load.id, error: { code: -32602, message: "Session stale-session is already loaded" } }));
    const created = await request("session/new");
    reply(created.id, { sessionId: "fresh-session" });
    await selectModelAndMode("fresh-session", "auto");
    const prompt = await request("session/prompt");
    reply(prompt.id, { stopReason: "end_turn" });
    await turn;
    expect(events).toContainEqual({ type: "session.providerBound", providerSessionId: "fresh-session" });
    await stopCopilotSession("copilot-stale");
  });

  it("resumes a bound session after stop and starts fresh after forget", async () => {
    const first = start("copilot-resume", []);
    await ready();
    await selectModelAndMode("copilot-session-1", "auto");
    const firstPrompt = await request("session/prompt");
    reply(firstPrompt.id, { stopReason: "end_turn" });
    await first;

    await stopCopilotSession("copilot-resume");
    sent.length = 0;

    const second = start("copilot-resume", []);
    const reinit = await request("initialize");
    reply(reinit.id, { protocolVersion: 1 });
    const load = await request("session/load");
    expect(load.params).toEqual({
      sessionId: "copilot-session-1",
      cwd: "/repo",
      mcpServers: [],
    });
    reply(load.id, { models: { currentModelId: "claude-sonnet-5" } });
    await selectModelAndMode("copilot-session-1", "auto");
    const secondPrompt = await request("session/prompt");
    reply(secondPrompt.id, { stopReason: "end_turn" });
    await second;
    expect(parse().some((message) => message.method === "session/new")).toBe(false);

    await forgetCopilotSession("copilot-resume");
    sent.length = 0;

    const third = start("copilot-resume", []);
    const init = await request("initialize");
    reply(init.id, { protocolVersion: 1 });
    const created = await request("session/new");
    reply(created.id, { sessionId: "copilot-session-2" });
    await selectModelAndMode("copilot-session-2", "auto");
    const thirdPrompt = await request("session/prompt");
    reply(thirdPrompt.id, { stopReason: "end_turn" });
    await third;
    await stopCopilotSession("copilot-resume");
  });

  it("denies a permission request that arrives after cancel", async () => {
    const events: HarnessEvent[] = [];
    const turn = start("copilot-late-permission", events);
    await ready();
    await selectModelAndMode("copilot-session-1", "auto");
    await request("session/prompt");
    await cancelCopilotTurn("copilot-late-permission");
    await turn;
    sent.length = 0;

    onLine!(JSON.stringify({
      jsonrpc: "2.0",
      id: 77,
      method: "session/request_permission",
      params: {
        sessionId: "copilot-session-1",
        toolCall: { toolCallId: "call-late", title: "Create file", kind: "edit", status: "pending" },
        options: [
          { optionId: "allow_once", kind: "allow_once", name: "Allow once" },
          { optionId: "reject_once", kind: "reject_once", name: "Deny" },
        ],
      },
    }));
    await vi.waitFor(() =>
      expect(parse().some((message) => message.id === 77 && message.result)).toBe(true),
    );
    expect(parse().find((message) => message.id === 77)?.result).toEqual({
      outcome: { outcome: "selected", optionId: "reject_once" },
    });
    expect(events.some((event) => event.type === "approval.requested")).toBe(false);
    await stopCopilotSession("copilot-late-permission");
  });

  it("keeps an unclassified request behind approval in auto-accept-edits", async () => {
    const events: HarnessEvent[] = [];
    const turn = start("copilot-auto", events, {
      runtimeMode: "auto-accept-edits",
    });
    await ready();
    await selectModelAndMode("copilot-session-1", "auto");
    const prompt = await request("session/prompt");

    onLine!(JSON.stringify({
      jsonrpc: "2.0",
      id: 55,
      method: "session/request_permission",
      params: {
        sessionId: "copilot-session-1",
        toolCall: { toolCallId: "call-unknown", title: "Do something", status: "pending" },
        options: [
          { optionId: "allow_once", kind: "allow_once", name: "Allow once" },
          { optionId: "reject_once", kind: "reject_once", name: "Deny" },
        ],
      },
    }));
    expect(events).toContainEqual(
      expect.objectContaining({ type: "approval.requested", requestId: 55 }),
    );
    respondCopilotApproval("copilot-auto", 55, "deny");
    await vi.waitFor(() =>
      expect(parse().some((message) => message.id === 55 && message.result)).toBe(true),
    );
    expect(parse().find((message) => message.id === 55)?.result).toEqual({
      outcome: { outcome: "selected", optionId: "reject_once" },
    });
    reply(prompt.id, { stopReason: "end_turn" });
    await turn;
    await stopCopilotSession("copilot-auto");
  });

  it("cancels pending prompts and suppresses late updates", async () => {
    const events: HarnessEvent[] = [];
    const turn = start("copilot-cancel", events);
    await ready();
    await selectModelAndMode("copilot-session-1", "auto");
    await request("session/prompt");
    await cancelCopilotTurn("copilot-cancel");
    await turn;
    expect(parse()).toContainEqual(expect.objectContaining({ method: "session/cancel", params: { sessionId: "copilot-session-1" } }));
    onStderr?.("[INFO] authentication failed");
    expect(events.some((event) => event.type === "session.error")).toBe(false);
    onExit?.(0);
    expect(events).toContainEqual({ type: "session.ended", code: 0 });
    await stopCopilotSession("copilot-cancel");
  });

  it("stops a session that finishes starting after the turn was cancelled", async () => {
    const events: HarnessEvent[] = [];
    const sessionId = "copilot-startup-cancel";
    const turn = start(sessionId, events);
    const init = await request("initialize");
    await cancelCopilotTurn(sessionId);
    reply(init.id, { protocolVersion: 1 });
    const created = await request("session/new");
    reply(created.id, { sessionId: "copilot-session-1" });
    await turn;
    expect(kill).toHaveBeenCalledWith(sessionId);
  });

  it("honors cancellation from session.started without cancelling the next turn", async () => {
    const sessionId = "copilot-started-cancel";
    let settled = false;
    const turn = start(sessionId, [], {
      /** Cancels synchronously when startup announces the session. */
      onEvent: (event) => {
        if (event.type === "session.started") void cancelCopilotTurn(sessionId);
      },
    }).then(
      () => { settled = true; },
      (error: unknown) => { settled = true; return error; },
    );
    try {
      await ready();
      await vi.waitFor(() => expect(settled).toBe(true));
      const result = await turn;
      expect(parse().some((message) => message.method === "session/set_model" || message.method === "session/set_mode" || message.method === "session/prompt")).toBe(false);
      expect(result).toBeUndefined();
      expect(kill).toHaveBeenCalledWith(sessionId);
      sent.length = 0;

      const next = start(sessionId, []);
      const init = await request("initialize");
      reply(init.id, { protocolVersion: 1 });
      const load = await request("session/load");
      reply(load.id, { sessionId: "copilot-session-1" });
      await selectModelAndMode("copilot-session-1", "auto");
      const prompt = await request("session/prompt");
      reply(prompt.id, { stopReason: "end_turn" });
      await next;
    } finally {
      await cancelCopilotTurn(sessionId);
      await turn;
      await stopCopilotSession(sessionId);
    }
  });

  it("drops sends queued before cancel without rejecting a fresh send after a slow cancel write", async () => {
    const sessionId = "copilot-queued-cancel";
    const first = start(sessionId, []);
    await ready();
    await selectModelAndMode("copilot-session-1", "auto");
    await request("session/prompt");
    const queued = start(sessionId, [], { text: "must not run" });
    let releaseCancel!: () => void;
    cancelWrite = new Promise<void>((resolve) => { releaseCancel = resolve; });
    const cancelled = cancelCopilotTurn(sessionId);
    let freshSettled = false;
    const fresh = start(sessionId, [], { text: "fresh turn" }).then(
      () => { freshSettled = true; },
      (error: unknown) => { freshSettled = true; return error; },
    );
    try {
      await Promise.all([first, queued]);
      await vi.waitFor(() => expect(parse().filter((message) => message.method === "session/prompt")).toHaveLength(2));
      const prompt = parse().filter((message) => message.method === "session/prompt")[1];
      expect(prompt.params.prompt).toEqual([{ type: "text", text: "fresh turn" }]);
      releaseCancel();
      await cancelled;
      await new Promise((resolve) => setTimeout(resolve, 0));
      expect(freshSettled).toBe(false);
      reply(prompt.id, { stopReason: "end_turn" });
      expect(await fresh).toBeUndefined();
    } finally {
      releaseCancel();
      await cancelCopilotTurn(sessionId);
      await Promise.all([first, queued, fresh, cancelled]);
      await stopCopilotSession(sessionId);
    }
  });

  it("keeps a queued send from changing the active turn's permission policy", async () => {
    const events: HarnessEvent[] = [];
    const sessionId = "copilot-queued-policy";
    const first = start(sessionId, events, { intent: "plan" });
    await ready();
    await selectModelAndMode("copilot-session-1", "auto", "plan");
    const prompt = await request("session/prompt");
    const second = start(sessionId, [], { runtimeMode: "full-access" });

    onLine!(JSON.stringify({
      jsonrpc: "2.0",
      id: 91,
      method: "session/request_permission",
      params: {
        sessionId: "copilot-session-1",
        toolCall: { toolCallId: "call-exec", title: "Run tests", kind: "execute", status: "pending" },
        options: [
          { optionId: "allow_always", kind: "allow_always", name: "Always allow" },
          { optionId: "reject_once", kind: "reject_once", name: "Deny" },
        ],
      },
    }));
    await vi.waitFor(() => expect(parse().some((message) => message.id === 91 && message.result)).toBe(true));
    expect(parse().find((message) => message.id === 91)?.result).toEqual({
      outcome: { outcome: "selected", optionId: "reject_once" },
    });

    reply(prompt.id, { stopReason: "end_turn" });
    await first;
    sent.length = 0;
    const mode = await request("session/set_mode");
    reply(mode.id, {});
    const queued = await request("session/prompt");
    reply(queued.id, { stopReason: "end_turn" });
    await second;
    await stopCopilotSession(sessionId);
  });

  it("denies a permission request that arrives with no active prompt", async () => {
    const events: HarnessEvent[] = [];
    const sessionId = "copilot-idle-permission";
    const turn = start(sessionId, events);
    await ready();
    await selectModelAndMode("copilot-session-1", "auto");
    const prompt = await request("session/prompt");
    reply(prompt.id, { stopReason: "end_turn" });
    await turn;
    sent.length = 0;

    onLine!(JSON.stringify({
      jsonrpc: "2.0",
      id: 88,
      method: "session/request_permission",
      params: {
        sessionId: "copilot-session-1",
        toolCall: { toolCallId: "call-idle", title: "Create file", kind: "edit", status: "pending" },
        options: [
          { optionId: "allow_once", kind: "allow_once", name: "Allow once" },
          { optionId: "reject_once", kind: "reject_once", name: "Deny" },
        ],
      },
    }));
    await vi.waitFor(() => expect(parse().some((message) => message.id === 88 && message.result)).toBe(true));
    expect(parse().find((message) => message.id === 88)?.result).toEqual({
      outcome: { outcome: "selected", optionId: "reject_once" },
    });
    expect(events.some((event) => event.type === "approval.requested")).toBe(false);
    await stopCopilotSession(sessionId);
  });
});
