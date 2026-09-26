import { beforeEach, describe, expect, it, vi } from "vitest";
import type { HarnessEvent, SendTurnInput } from "../../core/types";

const sent: string[] = [];
let onLine: ((line: string) => void) | undefined;
let onExit: ((code: number | null) => void) | undefined;
let onStderr: ((line: string) => void) | undefined;
const spawn = vi.fn(async (..._args: unknown[]) => undefined);

vi.mock("../../core/child", () => ({
  resolveCopilotBinary: async () => ({ path: "/fake/copilot" }),
  spawnChild: (...args: Parameters<typeof spawn>) => spawn(...args),
  killChild: async () => undefined,
  unwatchChild: () => undefined,
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
  writeChild: async (_id: string, line: string) => {
    sent.push(line);
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

const parse = () => sent.map((line) => JSON.parse(line));

function reply(id: number, result: unknown) {
  onLine!(JSON.stringify({ jsonrpc: "2.0", id, result }));
}

async function request(method: string) {
  await vi.waitFor(() => expect(parse().some((message) => message.method === method)).toBe(true));
  return parse().find((message) => message.method === method)!;
}

function start(sessionId: string, events: HarnessEvent[], changes: Partial<SendTurnInput> = {}) {
  return sendCopilotTurn({
    sessionId,
    cwd: "/repo",
    model: "copilot:auto",
    runtimeMode: "supervised",
    text: "create file",
    attachments: [],
    onEvent: (event) => events.push(event),
    ...changes,
  });
}

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

async function selectModelAndMode(sessionId: string, modelId: string) {
  const model = await request("session/set_model");
  expect(model.params).toEqual({ sessionId, modelId });
  reply(model.id, {});
  const mode = await request("session/set_mode");
  expect(mode.params.modeId).toBe("https://agentclientprotocol.com/protocol/session-modes#agent");
  reply(mode.id, {});
}

describe("Copilot live ACP sequence", () => {
  beforeEach(() => {
    sent.length = 0;
    onLine = undefined;
    onExit = undefined;
    onStderr = undefined;
    spawn.mockClear();
  });

  it("rejects steering without sending a concurrent prompt or disturbing the active turn", async () => {
    const events: HarnessEvent[] = [];
    const turn = start("copilot-steer", events);
    await ready();
    await selectModelAndMode("copilot-session-1", "auto");
    await request("session/prompt");

    const steering = steerCopilotTurn({
      sessionId: "copilot-steer",
      cwd: "/repo",
      model: "copilot:auto",
      text: "follow up",
    }).catch((error: unknown) => error);
    const prompts = parse().filter((message) => message.method === "session/prompt");
    for (const prompt of prompts) reply(prompt.id, { stopReason: "end_turn" });
    await turn;
    const steeringResult = await steering;
    await stopCopilotSession("copilot-steer");

    expect(prompts).toHaveLength(1);
    expect(steeringResult).toEqual(new Error("Copilot CLI does not support steering an in-flight turn"));
    expect(events.filter((event) => event.type === "message.completed")).toHaveLength(1);
    expect(events.filter((event) => event.type === "reasoning.completed")).toHaveLength(1);
    expect(events.some((event) => event.type === "session.error")).toBe(false);
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
});
