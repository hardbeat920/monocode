import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { HarnessEvent, SendTurnInput } from "./types";
import { modelsFor, resetHarnessModelOverlays } from "../models";

const mock = vi.hoisted(() => ({
  listeners: new Map<string, (line: string) => void>(),
  sent: [] as { thread: string; id?: number; method?: string; params?: Record<string, unknown>; result?: unknown }[],
  spawn: vi.fn(async () => undefined),
  kill: vi.fn(async () => undefined),
  fail: new Set<string>(),
  autoPrompt: false,
}));
vi.mock("../fs", () => ({ homeDir: async () => "/home/test" }));
vi.mock("./child", () => ({
  resolveKimiBinary: async () => ({ path: "/fake/kimi" }),
  resolveAntigravityBinary: async () => ({ path: "/fake/agy_acp_server.par" }),
  spawnChild: mock.spawn,
  killChild: mock.kill,
  unwatchChild: (id: string) => mock.listeners.delete(id),
  watchChild: (id: string, line: (line: string) => void) => mock.listeners.set(id, line),
  writeChild: async (thread: string, line: string) => {
    const message = JSON.parse(line);
    mock.sent.push({ thread, ...message });
    if (!message.method || message.id == null) return;
    if (message.method === "session/prompt" && !mock.autoPrompt) return;
    queueMicrotask(() => {
      const method = message.method;
      if (mock.fail.has(method)) {
        mock.listeners.get(thread)?.(JSON.stringify({ jsonrpc: "2.0", id: message.id,
          error: { code: -32601, message: method === "session/new" ? "Authentication required" : "unsupported" },
        }));
        return;
      }
      if (method === "session/load") {
        mock.listeners.get(thread)?.(JSON.stringify({ jsonrpc: "2.0", method: "session/update",
          params: { update: { sessionUpdate: "agent_message_chunk", content: { text: "OLD HISTORY" } } },
        }));
      }
      const setup = {
        sessionId: "provider-session",
        configOptions: [
          { id: "model", category: "model", currentValue: "m1", options: [
            { value: "m1", name: "Model One" }, { value: "m2", name: "Model Two" },
          ] },
          { id: "thinking", category: "thought_level", currentValue: "low", options: [
            { value: "low", name: "Low" }, { value: "high", name: "High" },
          ] },
        ],
      };
      const result = ["session/new", "session/load", "session/resume"].includes(method)
        ? setup : {};
      mock.listeners.get(thread)?.(JSON.stringify({ jsonrpc: "2.0", id: message.id, result }));
    });
  },
}));

const kimi = await import("./kimi");
const agy = await import("./antigravity");
const { refreshKimiCatalog } = await import("./kimiCatalog");
const { refreshAntigravityCatalog } = await import("./antigravityCatalog");

const providers = [
  { id: "kimi", send: kimi.sendKimiTurn, cancel: kimi.cancelKimiTurn, stop: kimi.stopKimiSession,
    forget: kimi.forgetKimiSession, bind: kimi.bindKimiSession, respond: kimi.respondKimiApproval,
    refresh: refreshKimiCatalog, path: "/fake/kimi", args: ["acp"], plan: "plan", auth: "kimi login" },
  { id: "antigravity", send: agy.sendAntigravityTurn, cancel: agy.cancelAntigravityTurn,
    stop: agy.stopAntigravitySession, forget: agy.forgetAntigravitySession,
    bind: agy.bindAntigravitySession, respond: agy.respondAntigravityApproval,
    refresh: refreshAntigravityCatalog, path: "/fake/agy_acp_server.par", args: [], plan: "default", auth: "agy` once" },
] as const;

function permission(kind = "execute") {
  mock.listeners.get("thread")!(JSON.stringify({ jsonrpc: "2.0", id: 100,
    method: "session/request_permission", params: {
      toolCall: { toolCallId: "tool-1", title: "Do work", kind },
      options: [ { optionId: "yes", kind: "allow_once" }, { optionId: "no", kind: "reject_once" } ],
    },
  }));
}
function finishPrompt() {
  const prompt = mock.sent.findLast((message) => message.method === "session/prompt")!;
  mock.listeners.get("thread")!(JSON.stringify({ jsonrpc: "2.0", id: prompt.id, result: { stopReason: "end_turn" } }));
}
const response = () => mock.sent.findLast((message) => message.id === 100 && message.result)?.result;
const waitPrompt = () => vi.waitFor(() => expect(mock.sent.some((m) => m.method === "session/prompt")).toBe(true));

describe.each(providers)("$id offline ACP transport", (provider) => {
  let events: HarnessEvent[];
  let input: SendTurnInput;
  beforeEach(() => {
    mock.sent.length = 0;
    mock.fail.clear();
    mock.autoPrompt = false;
    mock.spawn.mockClear();
    mock.kill.mockClear();
    events = [];
    input = { sessionId: "thread", cwd: "/repo", model: `${provider.id}:m1`, text: "hi",
      runtimeMode: "supervised", onEvent: (event) => events.push(event) };
  });
  afterEach(async () => {
    await provider.forget("thread");
    resetHarnessModelOverlays();
  });

  it("spawns the exact endpoint, sends settings/images and routes real approvals", async () => {
    const turn = provider.send({ ...input, modelSettings: { effort: "high" }, attachments: [
      { id: "img", name: "img.png", kind: "image", mimeType: "image/png", size: 4, data: "aGV5" },
    ] });
    await waitPrompt();
    expect(mock.spawn).toHaveBeenCalledWith(
      "thread", provider.path, provider.args, provider.id === "antigravity" ? "/fake/" : "/repo",
    );
    expect(mock.sent.find((m) => m.method === "initialize")?.params).toMatchObject({ protocolVersion: 1 });
    expect(mock.sent.find((m) => m.method === "session/set_config_option")?.params)
      .toMatchObject({ configId: "thinking", value: "high" });
    expect(mock.sent.find((m) => m.method === "session/prompt")?.params?.prompt)
      .toMatchObject([{ type: "text", text: "hi" }, { type: "image", data: "aGV5" }]);
    expect(mock.sent.find((m) => m.method === "session/new")?.params?.cwd).toBe("/repo");
    permission();
    await vi.waitFor(() => expect(events.some((e) => e.type === "approval.requested")).toBe(true));
    expect(response()).toBeUndefined();
    provider.respond("thread", 100, "allow");
    await vi.waitFor(() => expect(response()).toEqual({ outcome: { outcome: "selected", optionId: "yes" } }));
    finishPrompt();
    await turn;
    expect(events).toContainEqual({ type: "session.providerBound", providerSessionId: "provider-session" });
    expect(events).toContainEqual({ type: "message.completed" });
  });

  it("denies edits in plan intent even with full access selected", async () => {
    const turn = provider.send({ ...input, runtimeMode: "full-access", intent: "plan" });
    await waitPrompt();
    expect(mock.sent.find((m) => m.method === "session/set_mode")?.params?.modeId).toBe(provider.plan);
    permission("edit");
    await vi.waitFor(() => expect(response()).toEqual({ outcome: { outcome: "selected", optionId: "no" } }));
    expect(events.some((e) => e.type === "approval.requested")).toBe(false);
    finishPrompt();
    await turn;
  });

  it("cancels a pending approval and suppresses late text", async () => {
    const turn = provider.send(input);
    await waitPrompt();
    permission();
    await vi.waitFor(() => expect(events.some((e) => e.type === "approval.requested")).toBe(true));
    await provider.cancel("thread");
    await turn;
    await vi.waitFor(() => expect(response()).toEqual({ outcome: { outcome: "cancelled" } }));
    mock.listeners.get("thread")!(JSON.stringify({ jsonrpc: "2.0", method: "session/update",
      params: { update: { sessionUpdate: "agent_message_chunk", content: { text: "late" } } },
    }));
    expect(events.some((e) => e.type === "message.delta")).toBe(false);
    expect(mock.sent.some((m) => m.method === "session/cancel")).toBe(true);
  });

  it("fails closed when the provider rejects mode selection", async () => {
    mock.fail.add("session/set_mode");
    await expect(provider.send(input)).rejects.toThrow("unsupported");
    expect(mock.sent.some((m) => m.method === "session/prompt")).toBe(false);
    expect(mock.kill).toHaveBeenCalledWith("thread");
  });

  it.each(["resume", "load", "new"])("uses the %s branch of session recovery without replay", async (branch) => {
    provider.bind("thread", "saved-session", "/repo");
    if (branch !== "resume") mock.fail.add("session/resume");
    if (branch === "new") mock.fail.add("session/load");
    mock.autoPrompt = true;
    await provider.send(input);
    const methods = mock.sent.map((m) => m.method);
    expect(methods).toContain("session/resume");
    expect(methods.includes("session/load")).toBe(branch !== "resume");
    expect(methods.includes("session/new")).toBe(branch === "new");
    expect(events.some((e) => e.type === "message.delta")).toBe(false);
  });

  it("parks and resumes, then forgets the provider binding", async () => {
    mock.autoPrompt = true;
    await provider.send(input);
    await provider.stop("thread");
    mock.sent.length = 0;
    await provider.send(input);
    expect(mock.sent.some((m) => m.method === "session/resume")).toBe(true);
    await provider.forget("thread");
    mock.sent.length = 0;
    await provider.send(input);
    expect(mock.sent.some((m) => m.method === "session/new")).toBe(true);
    expect(mock.sent.some((m) => m.method === "session/resume")).toBe(false);
  });

  it("adds actionable authentication help and cleans up failed setup", async () => {
    mock.fail.add("session/new");
    await expect(provider.send(input)).rejects.toThrow(provider.auth);
    expect(mock.kill).toHaveBeenCalledWith("thread");
  });

  it("probes catalogs over ACP once, kills the probe, and preserves models on failure", async () => {
    const first = provider.refresh();
    expect(provider.refresh()).toBe(first);
    await first;
    expect(modelsFor(provider.id).map((model) => model.nativeId)).toEqual(["m1", "m2"]);
    expect(mock.spawn).toHaveBeenCalledWith(
      `monocode-${provider.id}-probe`, provider.path, provider.args,
      provider.id === "antigravity" ? "/fake/" : "/home/test",
    );
    expect(mock.kill).toHaveBeenCalledWith(`monocode-${provider.id}-probe`);
    expect(mock.listeners.has(`monocode-${provider.id}-probe`)).toBe(false);
    mock.fail.add("session/new");
    await provider.refresh();
    expect(modelsFor(provider.id).map((model) => model.nativeId)).toEqual(["m1", "m2"]);
    expect(mock.sent.some((m) => m.method === "authenticate")).toBe(false);
  });
});
