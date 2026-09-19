import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { HarnessEvent, SendTurnInput } from "./types";
import { modelsFor, resetHarnessModelOverlays } from "../models";

const mock = vi.hoisted(() => {
  const listeners = new Map<string, (line: string) => void>();
  const exits = new Map<string, (code: number | null) => void>();
  return {
    listeners,
    exits,
    sent: [] as { thread: string; id?: number; method?: string; params?: Record<string, unknown>; result?: unknown }[],
    spawn: vi.fn(async () => undefined),
    // killChild unwatches in the real bridge; mirror that so a killed
    // generation can never deliver output again.
    kill: vi.fn(async (id: string) => {
      listeners.delete(id);
      exits.delete(id);
    }),
    fail: new Set<string>(),
    silent: new Set<string>(),
    autoPrompt: false,
    promptStop: "end_turn",
    blockWrites: false,
    resolveGates: null as Array<() => void> | null,
  };
});
vi.mock("../fs", () => ({ homeDir: async () => "/home/test" }));
vi.mock("./child", () => ({
  resolveKimiBinary: async () => {
    if (mock.resolveGates) {
      await new Promise<void>((resolve) => mock.resolveGates.push(resolve));
    }
    return { path: "/fake/kimi" };
  },
  spawnChild: mock.spawn,
  killChild: mock.kill,
  unwatchChild: (id: string) => {
    mock.listeners.delete(id);
    mock.exits.delete(id);
  },
  watchChild: (id: string, line: (line: string) => void, exit?: (code: number | null) => void) => {
    mock.listeners.set(id, line);
    if (exit) mock.exits.set(id, exit);
  },
  writeChild: async (thread: string, line: string) => {
    const message = JSON.parse(line);
    mock.sent.push({ thread, ...message });
    if (mock.blockWrites) return new Promise<void>(() => undefined);
    if (!message.method || message.id == null) return;
    if (mock.silent.has(message.method)) return;
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
        ? setup
        : method === "session/prompt"
          ? { stopReason: mock.promptStop }
          : {};
      mock.listeners.get(thread)?.(JSON.stringify({ jsonrpc: "2.0", id: message.id, result }));
    });
  },
}));

const kimi = await import("./kimi");
const { refreshKimiCatalog } = await import("./kimiCatalog");

const providers = [
  { id: "kimi", send: kimi.sendKimiTurn, cancel: kimi.cancelKimiTurn,
    stop: kimi.stopKimiSession, forget: kimi.forgetKimiSession,
    bind: kimi.bindKimiSession, respond: kimi.respondKimiApproval,
    refresh: refreshKimiCatalog, path: "/fake/kimi", args: ["acp"], plan: "plan", auth: "kimi login" },
] as const;

// Each spawned generation registers under a scoped child key `thread#n`.
const childKeys = () =>
  [...mock.listeners.keys()].filter((key) => key.startsWith("thread#"));
const liveKey = () => childKeys().at(-1)!;
const childListener = () => mock.listeners.get(liveKey())!;
const genKey = expect.stringMatching(/^thread#\d+$/);

function permission(kind = "execute") {
  childListener()(JSON.stringify({ jsonrpc: "2.0", id: 100,
    method: "session/request_permission", params: {
      toolCall: { toolCallId: "tool-1", title: "Do work", kind },
      options: [ { optionId: "yes", kind: "allow_once" }, { optionId: "no", kind: "reject_once" } ],
    },
  }));
}
function finishPrompt() {
  const prompt = mock.sent.findLast((message) => message.method === "session/prompt")!;
  childListener()(JSON.stringify({ jsonrpc: "2.0", id: prompt.id, result: { stopReason: mock.promptStop } }));
}
const response = () => mock.sent.findLast((message) => message.id === 100 && message.result)?.result;
const waitPrompt = () => vi.waitFor(() => expect(mock.sent.some((m) => m.method === "session/prompt")).toBe(true));
const flush = async () => {
  for (let i = 0; i < 100; i++) await Promise.resolve();
};

describe.each(providers)("$id offline ACP transport", (provider) => {
  let events: HarnessEvent[];
  let input: SendTurnInput;
  beforeEach(() => {
    mock.sent.length = 0;
    mock.fail.clear();
    mock.silent.clear();
    mock.autoPrompt = false;
    mock.promptStop = "end_turn";
    mock.blockWrites = false;
    mock.resolveGates = null;
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
      genKey, provider.path, provider.args, "/repo",
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
    childListener()(JSON.stringify({ jsonrpc: "2.0", method: "session/update",
      params: { update: { sessionUpdate: "agent_message_chunk", content: { text: "late" } } },
    }));
    expect(events.some((e) => e.type === "message.delta")).toBe(false);
    expect(mock.sent.some((m) => m.method === "session/cancel")).toBe(true);
  });

  it("fails closed when the provider rejects mode selection", async () => {
    mock.fail.add("session/set_mode");
    await expect(provider.send(input)).rejects.toThrow("unsupported");
    expect(mock.sent.some((m) => m.method === "session/prompt")).toBe(false);
    expect(mock.kill).toHaveBeenCalledWith(genKey);
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
    expect(mock.kill).toHaveBeenCalledWith(genKey);
  });

  it("probes catalogs over ACP once, kills the probe, and preserves models on failure", async () => {
    const first = provider.refresh();
    expect(provider.refresh()).toBe(first);
    await first;
    expect(modelsFor(provider.id).map((model) => model.nativeId)).toEqual(["m1", "m2"]);
    expect(mock.spawn).toHaveBeenCalledWith(
      `monocode-${provider.id}-probe`, provider.path, provider.args,
      "/home/test",
    );
    expect(mock.kill).toHaveBeenCalledWith(`monocode-${provider.id}-probe`);
    expect(mock.listeners.has(`monocode-${provider.id}-probe`)).toBe(false);
    mock.fail.add("session/new");
    await provider.refresh();
    expect(modelsFor(provider.id).map((model) => model.nativeId)).toEqual(["m1", "m2"]);
    expect(mock.sent.some((m) => m.method === "authenticate")).toBe(false);
  });

  it("serializes two concurrent first sends through one startup", async () => {
    mock.autoPrompt = true;
    const secondEvents: HarnessEvent[] = [];
    await Promise.all([
      provider.send(input),
      provider.send({ ...input, text: "second", onEvent: (e) => secondEvents.push(e) }),
    ]);
    expect(mock.spawn).toHaveBeenCalledTimes(1);
    expect(mock.sent.filter((m) => m.method === "session/prompt")).toHaveLength(2);
    expect(secondEvents).toContainEqual({ type: "message.completed" });
  });

  it("keeps the running turn's listener and policy while a send is queued", async () => {
    const first = provider.send(input);
    await waitPrompt();
    const secondEvents: HarnessEvent[] = [];
    const second = provider.send({
      ...input, text: "next", runtimeMode: "full-access",
      onEvent: (e) => secondEvents.push(e),
    });
    permission();
    // The queued send's full-access mode must not auto-approve turn 1's prompt,
    // and turn 2's listener must not see turn 1's approval card.
    await vi.waitFor(() => expect(events.some((e) => e.type === "approval.requested")).toBe(true));
    expect(secondEvents.some((e) => e.type === "approval.requested")).toBe(false);
    expect(response()).toBeUndefined();
    provider.respond("thread", 100, "allow");
    await vi.waitFor(() => expect(response()).toBeTruthy());
    finishPrompt();
    await first;
    await vi.waitFor(() => expect(mock.sent.filter((m) => m.method === "session/prompt")).toHaveLength(2));
    finishPrompt();
    await second;
    expect(secondEvents).toContainEqual({ type: "message.completed" });
  });

  it("recycles the transport after a mid-prompt cancel instead of reusing it", async () => {
    const first = provider.send(input);
    await waitPrompt();
    await provider.cancel("thread");
    await first;
    mock.autoPrompt = true;
    mock.sent.length = 0;
    await provider.send({ ...input, text: "again" });
    expect(mock.kill).toHaveBeenCalledWith(genKey);
    expect(mock.spawn).toHaveBeenCalledTimes(2);
    const methods = mock.sent.map((m) => m.method);
    expect(methods).toContain("session/resume");
    expect(methods).toContain("session/prompt");
    expect(events).toContainEqual({ type: "message.completed" });
  });

  it("reuses the live transport when a cancel landed between turns", async () => {
    mock.autoPrompt = true;
    await provider.send(input);
    await provider.cancel("thread");
    mock.sent.length = 0;
    await provider.send({ ...input, text: "again" });
    expect(mock.spawn).toHaveBeenCalledTimes(1);
    expect(mock.sent.some((m) => m.method === "session/resume")).toBe(false);
    expect(mock.sent.filter((m) => m.method === "session/prompt")).toHaveLength(1);
  });

  it("flags a wedged prompt after two quiet minutes and recovers on resend", async () => {
    vi.useFakeTimers();
    try {
      const turn = provider.send(input);
      await flush();
      expect(mock.sent.some((m) => m.method === "session/prompt")).toBe(true);
      // Traffic resets the silence clock.
      await vi.advanceTimersByTimeAsync(119_000);
      childListener()(JSON.stringify({ jsonrpc: "2.0", method: "session/update",
        params: { update: { sessionUpdate: "agent_message_chunk", content: { text: "still here" } } },
      }));
      await vi.advanceTimersByTimeAsync(119_000);
      expect(events.some((e) => e.type === "status")).toBe(false);
      await vi.advanceTimersByTimeAsync(2_000);
      expect(events.some((e) => e.type === "status" && /quiet/i.test(e.text))).toBe(true);
      // A pending approval is waiting on the user, not a stalled server: the
      // next turn after cancel must recycle onto a fresh transport.
      await provider.cancel("thread");
      await turn;
      vi.useRealTimers();
      mock.autoPrompt = true;
      await provider.send({ ...input, text: "recover" });
      expect(mock.spawn).toHaveBeenCalledTimes(2);
      expect(mock.sent.some((m) => m.method === "session/resume")).toBe(true);
    } finally {
      vi.useRealTimers();
    }
  });

  it("settles a pending approval and the turn when the process exits", async () => {
    const turn = provider.send(input);
    await waitPrompt();
    permission();
    await vi.waitFor(() => expect(events.some((e) => e.type === "approval.requested")).toBe(true));
    mock.exits.get(liveKey())!(1);
    await turn;
    await vi.waitFor(() =>
      expect(response()).toEqual({ outcome: { outcome: "cancelled" } }));
    expect(events).toContainEqual({ type: "session.ended", code: 1 });
    expect(events).toContainEqual({ type: "approval.resolved", requestId: 100, decision: "deny" });
  });

  it("fails a resume timeout instead of stacking a fresh session on top", async () => {
    provider.bind("thread", "saved-session", "/repo");
    mock.silent.add("session/resume");
    vi.useFakeTimers();
    try {
      const send = provider.send(input);
      const outcome = send.then(() => "resolved").catch((e: Error) => e.message);
      await vi.advanceTimersByTimeAsync(50_000);
      await expect(outcome).resolves.toMatch(/timed out/);
      expect(mock.sent.some((m) => m.method === "session/load")).toBe(false);
      expect(mock.sent.some((m) => m.method === "session/new")).toBe(false);
    } finally {
      vi.useRealTimers();
    }
  });

  it("treats cancelled and refused stop reasons as ended turns, not completions", async () => {
    mock.autoPrompt = true;
    mock.promptStop = "cancelled";
    await provider.send(input);
    expect(events.some((e) => e.type === "message.completed")).toBe(false);
    mock.promptStop = "refusal";
    await provider.send(input);
    expect(events.some((e) => e.type === "session.error" && /refusal/.test(e.message))).toBe(true);
    mock.promptStop = "max_tokens";
    await provider.send(input);
    expect(events.some((e) => e.type === "session.error" && /max_tokens/.test(e.message))).toBe(true);
    expect(events.filter((e) => e.type === "message.completed")).toHaveLength(0);
  });

  it("stays quiet past the watchdog while an approval awaits the user", async () => {
    vi.useFakeTimers();
    try {
      const turn = provider.send(input);
      await flush();
      permission();
      await flush();
      expect(events.some((e) => e.type === "approval.requested")).toBe(true);
      // The approval wait is user time, not provider silence: no stall note.
      await vi.advanceTimersByTimeAsync(300_000);
      expect(events.some((e) => e.type === "status")).toBe(false);
      provider.respond("thread", 100, "allow");
      await flush();
      finishPrompt();
      await vi.runAllTimersAsync();
      await turn;
      expect(events).toContainEqual({ type: "message.completed" });
    } finally {
      vi.useRealTimers();
    }
  });

  it("recycles a prompt that outlives the hard timeout and resumes once", async () => {
    vi.useFakeTimers();
    try {
      const turn = provider.send(input);
      await flush();
      const outcome = turn.then(() => "resolved").catch((e: Error) => e.message);
      await vi.advanceTimersByTimeAsync(31 * 60_000);
      await expect(outcome).resolves.toMatch(/timed out/);
    } finally {
      vi.useRealTimers();
    }
    mock.autoPrompt = true;
    mock.sent.length = 0;
    await provider.send({ ...input, text: "retry" });
    expect(mock.spawn).toHaveBeenCalledTimes(2);
    const methods = mock.sent.map((m) => m.method);
    expect(methods.filter((m) => m === "session/resume")).toHaveLength(1);
    expect(methods).not.toContain("session/new");
  });

  it("ignores a retired process emitting after the transport was recycled", async () => {
    const first = provider.send(input);
    await waitPrompt();
    const oldKey = liveKey();
    const oldListener = mock.listeners.get(oldKey)!;
    await provider.cancel("thread");
    await first;
    mock.autoPrompt = true;
    const secondEvents: HarnessEvent[] = [];
    const second = provider.send({
      ...input, text: "next", onEvent: (e) => secondEvents.push(e),
    });
    await vi.waitFor(() => expect(mock.spawn).toHaveBeenCalledTimes(2));
    // The replacement runs under a new generation key: the retired child's
    // buffered stdout has no registered route to the new ACP client, and an
    // in-process emission through the stale listener stays muted.
    expect(liveKey()).not.toBe(oldKey);
    expect(mock.listeners.has(oldKey)).toBe(false);
    oldListener(JSON.stringify({ jsonrpc: "2.0", method: "session/update",
      params: { update: { sessionUpdate: "agent_message_chunk", content: { text: "stale" } } },
    }));
    await second;
    expect(secondEvents.some((e) => e.type === "message.delta" && e.text === "stale")).toBe(false);
    expect(secondEvents).toContainEqual({ type: "message.completed" });
  });

  it("suppresses a send that was queued when the running turn was cancelled", async () => {
    const first = provider.send(input);
    await waitPrompt();
    const secondEvents: HarnessEvent[] = [];
    const second = provider.send({
      ...input, text: "queued", onEvent: (e) => secondEvents.push(e),
    });
    await provider.cancel("thread");
    await first;
    await second;
    // The queued send was pending at cancel time: it must not prompt later.
    expect(mock.sent.filter((m) => m.method === "session/prompt")).toHaveLength(1);
    // A send issued after the cancel proceeds on a recycled transport.
    mock.autoPrompt = true;
    await provider.send({ ...input, text: "after" });
    expect(mock.spawn).toHaveBeenCalledTimes(2);
    expect(mock.sent.some((m) => m.method === "session/resume")).toBe(true);
    expect(mock.sent.filter((m) => m.method === "session/prompt")).toHaveLength(2);
  });

  it("retires an in-flight startup when the session is cancelled or forgotten", async () => {
    for (const end of ["cancel", "forget"] as const) {
      let releaseSpawn: () => void = () => undefined;
      mock.spawn.mockImplementationOnce(
        () => new Promise<void>((resolve) => { releaseSpawn = resolve; }),
      );
      const send = provider.send(input);
      await vi.waitFor(() => expect(mock.spawn).toHaveBeenCalledTimes(1));
      if (end === "cancel") {
        await provider.cancel("thread");
      } else {
        await provider.forget("thread");
      }
      releaseSpawn();
      await send;
      // The retired startup kills only its own generation's child and never
      // publishes: no initialize or session setup reached the wire.
      expect(mock.kill).toHaveBeenCalledWith(genKey);
      expect(mock.sent.some((m) => m.method === "initialize")).toBe(false);
      mock.sent.length = 0;
      mock.spawn.mockClear();
      mock.kill.mockClear();
    }
    // A send after the forgotten session starts cleanly with a fresh session.
    mock.autoPrompt = true;
    await provider.send({ ...input, text: "again" });
    expect(mock.spawn).toHaveBeenCalledTimes(1);
    expect(mock.sent.some((m) => m.method === "session/new")).toBe(true);
  });

  it("lets the next waiter retry once a shared startup fails", async () => {
    mock.autoPrompt = true;
    mock.spawn.mockRejectedValueOnce(new Error("spawn boom"));
    const first = provider.send(input);
    const second = provider.send({ ...input, text: "second" });
    await expect(first).rejects.toThrow("spawn boom");
    await second;
    // The first send's failure does not poison the chain; the second send's
    // lifecycle step rechecks ownership and retries the spawn exactly once.
    expect(mock.spawn).toHaveBeenCalledTimes(2);
    expect(mock.sent.filter((m) => m.method === "session/prompt")).toHaveLength(1);
  });

  it("does not resurrect a session forgotten while sends were queued behind startup", async () => {
    let releaseSpawn: () => void = () => undefined;
    mock.spawn.mockImplementationOnce(
      () => new Promise<void>((resolve) => { releaseSpawn = resolve; }),
    );
    const first = provider.send(input);
    const second = provider.send({ ...input, text: "queued" });
    await vi.waitFor(() => expect(mock.spawn).toHaveBeenCalledTimes(1));
    await provider.forget("thread");
    releaseSpawn();
    await first;
    await second;
    // The queued lifecycle step was submitted against the pre-forget epoch:
    // it never reaches its own spawn, let alone session setup.
    expect(mock.spawn).toHaveBeenCalledTimes(1);
    expect(mock.sent.some((m) => m.method === "session/new")).toBe(false);
    mock.autoPrompt = true;
    await provider.send({ ...input, text: "after" });
    expect(mock.sent.some((m) => m.method === "session/new")).toBe(true);
  });

  it("queues a different-cwd send without interrupting the running turn", async () => {
    const first = provider.send(input);
    await waitPrompt();
    const second = provider.send({ ...input, cwd: "/other", text: "queued" });
    await flush();
    // The eager path must not tear down the live turn for another cwd.
    expect(mock.kill).not.toHaveBeenCalled();
    finishPrompt();
    await first;
    expect(events).toContainEqual({ type: "message.completed" });
    // Once it owns the turn, the queued send recycles onto its own cwd: a
    // moved cwd drops the resume binding and starts a fresh session.
    mock.autoPrompt = true;
    mock.sent.length = 0;
    await second;
    expect(mock.spawn).toHaveBeenCalledTimes(2);
    expect(mock.sent.some((m) => m.method === "session/new")).toBe(true);
    expect(mock.sent.some((m) => m.method === "session/resume")).toBe(false);
  });

  it("recycles the transport when a cancel lands during configuration", async () => {
    mock.silent.add("session/set_config_option");
    const first = provider.send({ ...input, model: `${provider.id}:m2` });
    await vi.waitFor(() =>
      expect(mock.sent.some((m) => m.method === "session/set_config_option")).toBe(true),
    );
    await provider.cancel("thread");
    await first;
    // The config request was in flight when cancelled: the transport is stale
    // and the next turn resumes on a fresh process rather than trusting the
    // old one's unknown server-side state.
    mock.autoPrompt = true;
    mock.sent.length = 0;
    await provider.send({ ...input, text: "again" });
    expect(mock.spawn).toHaveBeenCalledTimes(2);
    expect(mock.sent.some((m) => m.method === "session/resume")).toBe(true);
  });

  it("fails a turn whose stdin write blocks instead of hanging forever", async () => {
    vi.useFakeTimers();
    try {
      mock.blockWrites = true;
      const send = provider.send(input);
      const outcome = send.then(() => "resolved", (e: Error) => e.message);
      await vi.advanceTimersByTimeAsync(16_000);
      await expect(outcome).resolves.toMatch(/timed out/);
      // The wedged generation tears itself down so nothing reuses it.
      expect(mock.kill).toHaveBeenCalledWith(genKey);
    } finally {
      vi.useRealTimers();
      mock.blockWrites = false;
    }
  });

  it("settles the turn even when the cancel notification write blocks", async () => {
    const first = provider.send(input);
    await waitPrompt();
    mock.blockWrites = true;
    const started = Date.now();
    await provider.cancel("thread");
    await first;
    // rejectPending ran before the blocked session/cancel notify could hang us.
    expect(Date.now() - started).toBeLessThan(1_000);
    mock.blockWrites = false;
    mock.autoPrompt = true;
    await provider.send({ ...input, text: "again" });
    expect(mock.spawn).toHaveBeenCalledTimes(2);
    expect(mock.sent.some((m) => m.method === "session/resume")).toBe(true);
  });

  it("suppresses a queued send when the session is stopped", async () => {
    const first = provider.send(input);
    await waitPrompt();
    const second = provider.send({ ...input, text: "queued" });
    await provider.stop("thread");
    await first;
    await second;
    expect(mock.sent.filter((m) => m.method === "session/prompt")).toHaveLength(1);
    // Stop keeps the resume binding: the next turn rebinds on a fresh process.
    mock.autoPrompt = true;
    mock.sent.length = 0;
    await provider.send({ ...input, text: "after" });
    expect(mock.spawn).toHaveBeenCalledTimes(2);
    expect(mock.sent.some((m) => m.method === "session/resume")).toBe(true);
  });

  it("never spawns when forget lands while binary resolution is pending", async () => {
    mock.resolveGates = []; // truthy: resolution waits for release
    const send = provider.send(input);
    await vi.waitFor(() => expect(mock.resolveGates?.length).toBe(1));
    const release = () => {
      const gates = mock.resolveGates;
      mock.resolveGates = null;
      gates.forEach((open) => open());
    };
    // Forget while resolve is suspended: the abandoned startup must not reach
    // spawnChild at all.
    await provider.forget("thread");
    release();
    await send;
    expect(mock.spawn).not.toHaveBeenCalled();
    mock.autoPrompt = true;
    await provider.send({ ...input, text: "after" });
    expect(mock.spawn).toHaveBeenCalledTimes(1);
    expect(mock.sent.some((m) => m.method === "session/new")).toBe(true);
  });

  it("fails the turn when a permission reply cannot be written", async () => {
    const turn = provider.send(input);
    await waitPrompt();
    permission();
    await vi.waitFor(() => expect(events.some((e) => e.type === "approval.requested")).toBe(true));
    mock.blockWrites = true;
    vi.useFakeTimers();
    try {
      provider.respond("thread", 100, "allow");
      const outcome = turn.then(() => "resolved", (e: Error) => e.message);
      // The reply write wedges; the 15s bound fires and must retire the whole
      // generation — the provider is waiting for an answer that never left.
      await vi.advanceTimersByTimeAsync(16_000);
      await expect(outcome).resolves.toMatch(/timed out|not running/i);
      expect(mock.kill).toHaveBeenCalledWith(genKey);
    } finally {
      vi.useRealTimers();
      mock.blockWrites = false;
    }
  });
});
