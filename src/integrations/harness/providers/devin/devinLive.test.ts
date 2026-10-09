import { beforeEach, describe, expect, it, vi } from "vitest";

const sent: string[] = [];
let onLine: ((line: string) => void) | undefined;
const textFiles = new Map<string, string>();
const children = { spawned: 0, killed: 0, askEdits: [] as boolean[] };

vi.mock("../../core/child", () => ({
  resolveDevinBinary: async () => ({ path: "/fake/devin" }),
  spawnChild: async (...args: unknown[]) => {
    children.spawned += 1;
    children.askEdits.push(args[8] === true);
  },
  killChild: async () => {
    children.killed += 1;
  },
  unwatchChild: () => undefined,
  watchChild: (_id: string, line: (value: string) => void) => {
    onLine = line;
  },
  writeChild: async (_id: string, line: string) => {
    sent.push(line);
  },
  execChild: async () =>
    "Logged in.\n  Credentials path: /home/me/.local/share/devin/credentials.toml\n",
  readHarnessTextFile: async (path: string) => {
    const content = textFiles.get(path);
    if (content == null) throw new Error(`missing ${path}`);
    return content;
  },
}));

vi.mock("../../../../platform/tauri/fs", () => ({
  homeDir: async () => "/home/me",
}));

const {
  bindDevinSession,
  cancelDevinTurn,
  forgetDevinSession,
  respondDevinApproval,
  sendDevinTurn,
  stopDevinSession,
  waitForDevinSessionTitle,
} = await import("./devin");
const { resetDevinAuthCache } = await import("./devinAuth");
import type { HarnessEvent } from "../../core/types";

const parse = () => sent.map((line) => JSON.parse(line));
const request = (method: string) =>
  parse().find((message) => message.method === method);

function reply(id: number | string, result: unknown) {
  onLine!(JSON.stringify({ jsonrpc: "2.0", id, result }));
}

function notify(update: Record<string, unknown>) {
  onLine!(
    JSON.stringify({
      jsonrpc: "2.0",
      method: "session/update",
      params: { sessionId: "devin-1", update },
    }),
  );
}

async function waitFor(predicate: () => boolean, label: string) {
  for (let index = 0; index < 200; index += 1) {
    if (predicate()) return;
    await new Promise((resolve) => setTimeout(resolve, 5));
  }
  throw new Error(
    `timed out waiting for ${label}; sent=${JSON.stringify(parse().map((message) => message.method ?? `reply:${message.id}`))}`,
  );
}

async function answer(method: string, result: unknown) {
  await waitFor(() => !!request(method), method);
  reply(request(method)!.id, result);
}

const CONFIG = [
  { id: "mode", category: "mode", type: "select", currentValue: "accept-edits" },
  {
    id: "model",
    category: "model",
    type: "select",
    currentValue: "glm-5-2",
    options: [
      { value: "glm-5-2", name: "GLM-5.2 High" },
      { value: "swe-2-medium", name: "SWE-2 Medium" },
      {
        value: "fusion-claude-fable-5-1-medium-sidekick-swe-2-medium",
        name: "Fusion (Claude Fable 5.1 Medium + SWE-2 Medium)",
      },
      {
        value: "fusion-claude-fable-5-1-medium-sidekick-swe-2-high",
        name: "Fusion (Claude Fable 5.1 Medium + SWE-2 High)",
      },
    ],
  },
  {
    id: "thought_level",
    category: "thought_level",
    type: "select",
    currentValue: "high",
    options: [
      { value: "none", name: "No Thinking" },
      { value: "high", name: "High" },
      { value: "max", name: "Max" },
    ],
  },
];

async function startSession(load = false) {
  await answer("initialize", {
    protocolVersion: 1,
    authMethods: [{ id: "devin-browser" }],
  });
  await answer("authenticate", {});
  if (load) await answer("session/load", { configOptions: CONFIG });
  else await answer("session/new", { sessionId: "devin-1", configOptions: CONFIG });
}

describe("Devin live ACP sequence", () => {
  beforeEach(async () => {
    sent.length = 0;
    onLine = undefined;
    textFiles.clear();
    resetDevinAuthCache();
    await forgetDevinSession("devin-thread");
    children.spawned = 0;
    children.killed = 0;
    children.askEdits.length = 0;
  });

  it("makes a Supervised child ask before edits and restarts it to leave", async () => {
    const supervised = sendDevinTurn({
      sessionId: "devin-thread",
      cwd: "/repo",
      model: "devin:glm-5-2",
      runtimeMode: "supervised",
      text: "edit a file",
      onEvent: () => undefined,
    });
    await startSession();
    await answer("session/set_mode", {});
    await answer("session/prompt", { stopReason: "end_turn" });
    await supervised;
    expect(children.askEdits).toEqual([true]);

    // Same mode: the child is reused.
    sent.length = 0;
    const again = sendDevinTurn({
      sessionId: "devin-thread",
      cwd: "/repo",
      model: "devin:glm-5-2",
      runtimeMode: "supervised",
      text: "again",
      onEvent: () => undefined,
    });
    await answer("session/prompt", { stopReason: "end_turn" });
    await again;
    expect(children.spawned).toBe(1);

    // Leaving Supervised drops the rule and resumes the same conversation.
    sent.length = 0;
    const auto = sendDevinTurn({
      sessionId: "devin-thread",
      cwd: "/repo",
      model: "devin:glm-5-2",
      runtimeMode: "auto",
      text: "go",
      onEvent: () => undefined,
    });
    await startSession(true);
    expect(request("session/load")!.params.sessionId).toBe("devin-1");
    await answer("session/set_mode", {});
    await answer("session/prompt", { stopReason: "end_turn" });
    await auto;
    expect(children.askEdits).toEqual([true, false]);
  });

  it("reuses the CLI login, picks model and mode, and prompts", async () => {
    textFiles.set(
      "/home/me/.local/share/devin/credentials.toml",
      'windsurf_api_key = "devin-key"\n',
    );
    const events: HarnessEvent[] = [];
    const turn = sendDevinTurn({
      sessionId: "devin-thread",
      cwd: "/repo",
      model: "devin:swe-2-medium",
      runtimeMode: "auto",
      text: "hello",
      onEvent: (event) => events.push(event),
    });

    await startSession();
    expect(request("authenticate")!.params).toEqual({
      methodId: "devin-browser",
      _meta: { api_key: "devin-key" },
    });

    await answer("session/set_config_option", { configOptions: CONFIG });
    expect(request("session/set_config_option")!.params).toEqual({
      sessionId: "devin-1",
      configId: "model",
      value: "swe-2-medium",
    });
    await answer("session/set_mode", {});
    expect(request("session/set_mode")!.params.modeId).toBe("smart");

    await waitFor(() => !!request("session/prompt"), "session/prompt");
    notify({
      sessionUpdate: "agent_message_chunk",
      content: { type: "text", text: "hi there" },
    });
    reply(request("session/prompt")!.id, { stopReason: "end_turn" });
    await turn;

    expect(events).toContainEqual({
      type: "session.providerBound",
      providerSessionId: "devin-1",
    });
    expect(events).toContainEqual({ type: "message.delta", text: "hi there" });
    expect(events.some((event) => event.type === "session.error")).toBe(false);
  });

  it("keeps Devin's model when a saved one is no longer offered", async () => {
    const events: HarnessEvent[] = [];
    const turn = sendDevinTurn({
      sessionId: "devin-thread",
      cwd: "/repo",
      model: "devin:swe-1-7-lightning",
      runtimeMode: "auto",
      text: "hello",
      onEvent: (event) => events.push(event),
    });
    await startSession();
    await answer("session/set_mode", {});
    await answer("session/prompt", { stopReason: "end_turn" });
    await turn;
    expect(request("session/set_config_option")).toBeUndefined();
    expect(events).toContainEqual(
      expect.objectContaining({ type: "status", text: expect.stringContaining("swe-1-7-lightning") }),
    );
  });

  it("applies the picked effort through the thought_level option", async () => {
    const turn = sendDevinTurn({
      sessionId: "devin-thread",
      cwd: "/repo",
      model: "devin:glm-5-2",
      modelSettings: { effort: "max" },
      runtimeMode: "auto",
      text: "hello",
      onEvent: () => {},
    });
    await startSession();
    await answer("session/set_config_option", { configOptions: CONFIG });
    expect(request("session/set_config_option")!.params).toEqual({
      sessionId: "devin-1",
      configId: "thought_level",
      value: "max",
    });
    await answer("session/set_mode", {});
    await answer("session/prompt", { stopReason: "end_turn" });
    await turn;
  });

  it("maps a saved effort the model lacks to its nearest level", async () => {
    const turn = sendDevinTurn({
      sessionId: "devin-thread",
      cwd: "/repo",
      model: "devin:glm-5-2",
      modelSettings: { effort: "minimal" },
      runtimeMode: "auto",
      text: "hello",
      onEvent: () => {},
    });
    await startSession();
    await answer("session/set_config_option", { configOptions: CONFIG });
    // GLM-5.2 offers none/high/max: minimal becomes none, not a failed turn.
    expect(request("session/set_config_option")!.params).toEqual({
      sessionId: "devin-1",
      configId: "thought_level",
      value: "none",
    });
    expect(parse().filter((m) => m.method === "session/set_config_option")).toHaveLength(1);
    await answer("session/set_mode", {});
    await answer("session/prompt", { stopReason: "end_turn" });
    await turn;
  });

  it("ignores an effort that is not a level at all", async () => {
    const turn = sendDevinTurn({
      sessionId: "devin-thread",
      cwd: "/repo",
      model: "devin:glm-5-2",
      modelSettings: { effort: "turbo" },
      runtimeMode: "auto",
      text: "hello",
      onEvent: () => {},
    });
    await startSession();
    await answer("session/set_mode", {});
    await answer("session/prompt", { stopReason: "end_turn" });
    await turn;
    expect(request("session/set_config_option")).toBeUndefined();
  });

  it("applies effort against the levels of the model it switched to", async () => {
    const turn = sendDevinTurn({
      sessionId: "devin-thread",
      cwd: "/repo",
      model: "devin:swe-2-medium",
      modelSettings: { effort: "none" },
      runtimeMode: "auto",
      text: "hello",
      onEvent: () => {},
    });
    await startSession();
    // Switching to SWE-2 swaps the thought levels to medium/high/max.
    const swe2 = CONFIG.map((option) =>
      option.id === "model"
        ? { ...option, currentValue: "swe-2-medium" }
        : option.id === "thought_level"
          ? {
              ...option,
              currentValue: "medium",
              options: [
                { value: "medium", name: "Medium" },
                { value: "high", name: "High" },
                { value: "max", name: "Max" },
              ],
            }
          : option,
    );
    await answer("session/set_config_option", { configOptions: swe2 });
    expect(request("session/set_config_option")!.params.value).toBe("swe-2-medium");
    await answer("session/set_mode", {});
    await answer("session/prompt", { stopReason: "end_turn" });
    await turn;
    // "none" maps to medium, which is already current: nothing more is sent.
    expect(parse().filter((m) => m.method === "session/set_config_option")).toHaveLength(1);
  });

  it("restores a model saved under an older catalog's family key", async () => {
    const events: HarnessEvent[] = [];
    const turn = sendDevinTurn({
      sessionId: "devin-thread",
      cwd: "/repo",
      model: "devin:swe-2",
      runtimeMode: "auto",
      text: "hello",
      onEvent: (event) => events.push(event),
    });
    await startSession();
    await answer("session/set_config_option", { configOptions: CONFIG });
    expect(request("session/set_config_option")!.params.value).toBe("swe-2-medium");
    await answer("session/set_mode", {});
    await answer("session/prompt", { stopReason: "end_turn" });
    await turn;
    expect(events.some((event) => event.type === "status")).toBe(false);
  });

  it("resolves the fusion picker's lead and sidekick into one model id", async () => {
    const turn = sendDevinTurn({
      sessionId: "devin-thread",
      cwd: "/repo",
      model: "devin:fusion",
      modelSettings: { lead: "claude-fable-5-1", sidekick: "swe-2-high" },
      runtimeMode: "auto",
      text: "hello",
      onEvent: () => {},
    });
    await startSession();
    await answer("session/set_config_option", { configOptions: CONFIG });
    expect(request("session/set_config_option")!.params).toEqual({
      sessionId: "devin-1",
      configId: "model",
      value: "fusion-claude-fable-5-1-medium-sidekick-swe-2-high",
    });
    await answer("session/set_mode", {});
    await answer("session/prompt", { stopReason: "end_turn" });
    await turn;
  });

  it("answers string-id permission requests with the user's decision", async () => {
    const events: HarnessEvent[] = [];
    const turn = sendDevinTurn({
      sessionId: "devin-thread",
      cwd: "/repo",
      model: "devin:glm-5-2",
      runtimeMode: "supervised",
      text: "list files",
      onEvent: (event) => events.push(event),
    });
    await startSession();
    // No CLI login: Devin's own browser method is requested without a key.
    expect(request("authenticate")!.params).toEqual({ methodId: "devin-browser" });
    await answer("session/set_mode", {});
    expect(request("session/set_mode")!.params.modeId).toBe("accept-edits");
    await waitFor(() => !!request("session/prompt"), "session/prompt");

    notify({
      sessionUpdate: "tool_call",
      toolCallId: "call-1",
      title: "Ran ls",
      kind: "execute",
      rawInput: { command: "ls" },
    });
    onLine!(
      JSON.stringify({
        jsonrpc: "2.0",
        id: "7107bc27-2b44-4a89-a47b-8e4844d43eb3",
        method: "session/request_permission",
        params: {
          sessionId: "devin-1",
          toolCall: { toolCallId: "call-1", _meta: { "cognition.ai/editableCommand": "ls" } },
          options: [
            { optionId: "allow_once", name: "Allow", kind: "allow_once" },
            { optionId: "switch_bypass", name: "Bypass", kind: "allow_always" },
            { optionId: "reject_once", name: "Reject", kind: "reject_once" },
          ],
        },
      }),
    );
    await waitFor(
      () => events.some((event) => event.type === "approval.requested"),
      "approval.requested",
    );
    const asked = events.find((event) => event.type === "approval.requested");
    expect(asked).toMatchObject({ kind: "execute", callId: "call-1" });
    respondDevinApproval(
      "devin-thread",
      (asked as { requestId: number }).requestId,
      "allow",
    );
    await waitFor(
      () => parse().some((message) => message.id === "7107bc27-2b44-4a89-a47b-8e4844d43eb3"),
      "permission reply",
    );
    expect(
      parse().find((message) => message.id === "7107bc27-2b44-4a89-a47b-8e4844d43eb3")!.result,
    ).toEqual({ outcome: { outcome: "selected", optionId: "allow_once" } });

    reply(request("session/prompt")!.id, { stopReason: "end_turn" });
    await turn;
  });

  it("never answers a supervised allow with a persistent option", async () => {
    const events: HarnessEvent[] = [];
    const turn = sendDevinTurn({
      sessionId: "devin-thread",
      cwd: "/repo",
      model: "devin:glm-5-2",
      runtimeMode: "supervised",
      text: "list files",
      onEvent: (event) => events.push(event),
    });
    await startSession();
    await answer("session/set_mode", {});
    await waitFor(() => !!request("session/prompt"), "session/prompt");

    onLine!(
      JSON.stringify({
        jsonrpc: "2.0",
        id: "perm-no-once",
        method: "session/request_permission",
        params: {
          sessionId: "devin-1",
          toolCall: { toolCallId: "call-2" },
          options: [
            { optionId: "switch_bypass", name: "Bypass", kind: "allow_always" },
            { optionId: "reject_once", name: "Reject", kind: "reject_once" },
          ],
        },
      }),
    );
    await waitFor(
      () => events.some((event) => event.type === "approval.requested"),
      "approval.requested",
    );
    const asked = events.find((event) => event.type === "approval.requested");
    respondDevinApproval(
      "devin-thread",
      (asked as { requestId: number }).requestId,
      "allow",
    );
    await waitFor(
      () => parse().some((message) => message.id === "perm-no-once"),
      "permission reply",
    );
    expect(parse().find((message) => message.id === "perm-no-once")!.result).toEqual({
      outcome: { outcome: "cancelled" },
    });

    reply(request("session/prompt")!.id, { stopReason: "end_turn" });
    await turn;
  });

  it("ignores a cancel on an idle thread", async () => {
    await cancelDevinTurn("devin-thread");
    const turn = sendDevinTurn({
      sessionId: "devin-thread",
      cwd: "/repo",
      model: "devin:glm-5-2",
      runtimeMode: "auto",
      text: "hello",
      onEvent: () => undefined,
    });
    await startSession();
    await answer("session/set_mode", {});
    await waitFor(() => !!request("session/prompt"), "session/prompt");
    reply(request("session/prompt")!.id, { stopReason: "end_turn" });
    await turn;
  });

  it("honours a cancel that arrives while the child is starting", async () => {
    const turn = sendDevinTurn({
      sessionId: "devin-thread",
      cwd: "/repo",
      model: "devin:glm-5-2",
      runtimeMode: "auto",
      text: "hello",
      onEvent: () => undefined,
    });
    await waitFor(() => !!request("initialize"), "initialize");
    await cancelDevinTurn("devin-thread");
    await startSession();
    await turn;
    expect(request("session/set_mode")).toBeUndefined();
    expect(request("session/prompt")).toBeUndefined();
  });

  it("resumes with session/load and reports Devin's own title", async () => {
    bindDevinSession("devin-thread", "devin-1", "/repo");
    const events: HarnessEvent[] = [];
    const title = waitForDevinSessionTitle("devin-thread", 2_000);
    const turn = sendDevinTurn({
      sessionId: "devin-thread",
      cwd: "/repo",
      model: "devin:glm-5-2",
      runtimeMode: "full-access",
      text: "continue",
      onEvent: (event) => events.push(event),
    });
    await startSession(true);
    expect(request("session/load")!.params).toMatchObject({
      sessionId: "devin-1",
      cwd: "/repo",
    });
    expect(request("session/new")).toBeUndefined();
    await answer("session/set_mode", {});
    expect(request("session/set_mode")!.params.modeId).toBe("bypass");
    await waitFor(() => !!request("session/prompt"), "session/prompt");
    notify({ sessionUpdate: "session_info_update", title: "continue the..." });
    notify({ sessionUpdate: "session_info_update", title: "Resume repo work" });
    reply(request("session/prompt")!.id, { stopReason: "end_turn" });
    await turn;
    await expect(title).resolves.toBe("Resume repo work");
  });

  it("starts a new session when session/load fails", async () => {
    bindDevinSession("devin-thread", "gone-1", "/repo");
    const events: HarnessEvent[] = [];
    const turn = sendDevinTurn({
      sessionId: "devin-thread",
      cwd: "/repo",
      model: "devin:glm-5-2",
      runtimeMode: "auto",
      text: "continue",
      onEvent: (event) => events.push(event),
    });
    await answer("initialize", {
      protocolVersion: 1,
      authMethods: [{ id: "devin-browser" }],
    });
    await answer("authenticate", {});
    await waitFor(() => !!request("session/load"), "session/load");
    onLine!(
      JSON.stringify({
        jsonrpc: "2.0",
        id: request("session/load")!.id,
        error: { code: -32002, message: "Session not found" },
      }),
    );
    await answer("session/new", { sessionId: "devin-2", configOptions: CONFIG });
    await answer("session/set_mode", {});
    await waitFor(() => !!request("session/prompt"), "session/prompt");
    expect(request("session/prompt")!.params.sessionId).toBe("devin-2");
    expect(events).toContainEqual({
      type: "session.providerBound",
      providerSessionId: "devin-2",
    });
    expect(events.some((event) => event.type === "status")).toBe(true);
    reply(request("session/prompt")!.id, { stopReason: "end_turn" });
    await turn;
  });

  it("stops a child that is still starting without binding it", async () => {
    const events: HarnessEvent[] = [];
    const turn = sendDevinTurn({
      sessionId: "devin-thread",
      cwd: "/repo",
      model: "devin:glm-5-2",
      runtimeMode: "auto",
      text: "hello",
      onEvent: (event) => events.push(event),
    });
    await waitFor(() => !!request("initialize"), "initialize");
    await stopDevinSession("devin-thread");
    // The pending initialize is failed at once instead of waiting it out.
    await turn;
    expect(children.killed).toBeGreaterThan(0);
    expect(request("authenticate")).toBeUndefined();
    expect(request("session/new")).toBeUndefined();
    expect(events.some((event) => event.type === "session.providerBound")).toBe(false);
    expect(events.some((event) => event.type === "session.error")).toBe(false);
  });

  it("spawns nothing when stopped before the child exists", async () => {
    const turn = sendDevinTurn({
      sessionId: "devin-thread",
      cwd: "/repo",
      model: "devin:glm-5-2",
      runtimeMode: "auto",
      text: "hello",
      onEvent: () => undefined,
    });
    await stopDevinSession("devin-thread");
    await turn;
    expect(children.spawned).toBe(0);
    expect(sent).toHaveLength(0);
  });

  it("does not resurrect a session deleted mid-startup", async () => {
    bindDevinSession("devin-thread", "devin-1", "/repo");
    const events: HarnessEvent[] = [];
    const turn = sendDevinTurn({
      sessionId: "devin-thread",
      cwd: "/repo",
      model: "devin:glm-5-2",
      runtimeMode: "auto",
      text: "hello",
      onEvent: (event) => events.push(event),
    });
    await answer("initialize", { protocolVersion: 1, authMethods: [{ id: "devin-browser" }] });
    await answer("authenticate", {});
    await waitFor(() => !!request("session/load"), "session/load");
    await forgetDevinSession("devin-thread");
    reply(request("session/load")!.id, { configOptions: CONFIG });
    await turn;
    expect(events.some((event) => event.type === "session.providerBound")).toBe(false);

    // The deleted binding stays gone: the next turn starts fresh.
    sent.length = 0;
    const next = sendDevinTurn({
      sessionId: "devin-thread",
      cwd: "/repo",
      model: "devin:glm-5-2",
      runtimeMode: "auto",
      text: "again",
      onEvent: () => undefined,
    });
    await startSession();
    expect(request("session/load")).toBeUndefined();
    await answer("session/set_mode", {});
    await answer("session/prompt", { stopReason: "end_turn" });
    await next;
  });

  it("keeps the binding when session/load fails for another reason", async () => {
    bindDevinSession("devin-thread", "devin-1", "/repo");
    const events: HarnessEvent[] = [];
    const turn = sendDevinTurn({
      sessionId: "devin-thread",
      cwd: "/repo",
      model: "devin:glm-5-2",
      runtimeMode: "auto",
      text: "continue",
      onEvent: (event) => events.push(event),
    });
    await answer("initialize", { protocolVersion: 1, authMethods: [{ id: "devin-browser" }] });
    await answer("authenticate", {});
    await waitFor(() => !!request("session/load"), "session/load");
    onLine!(
      JSON.stringify({
        jsonrpc: "2.0",
        id: request("session/load")!.id,
        error: { code: -32603, message: "Internal error: database is locked" },
      }),
    );
    await expect(turn).rejects.toThrow(/could not restore.*database is locked/);
    expect(request("session/new")).toBeUndefined();
    expect(events.some((event) => event.type === "session.providerBound")).toBe(false);

    // The next turn retries the same conversation.
    sent.length = 0;
    const retry = sendDevinTurn({
      sessionId: "devin-thread",
      cwd: "/repo",
      model: "devin:glm-5-2",
      runtimeMode: "auto",
      text: "continue",
      onEvent: () => undefined,
    });
    await startSession(true);
    expect(request("session/load")!.params.sessionId).toBe("devin-1");
    await answer("session/set_mode", {});
    await answer("session/prompt", { stopReason: "end_turn" });
    await retry;
  });

  it("keeps the binding when session/load times out", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      bindDevinSession("devin-thread", "devin-1", "/repo");
      const events: HarnessEvent[] = [];
      const turn = sendDevinTurn({
        sessionId: "devin-thread",
        cwd: "/repo",
        model: "devin:glm-5-2",
        runtimeMode: "auto",
        text: "continue",
        onEvent: (event) => events.push(event),
      });
      const failed = expect(turn).rejects.toThrow(/timed out restoring/);
      await answer("initialize", { protocolVersion: 1, authMethods: [{ id: "devin-browser" }] });
      await answer("authenticate", {});
      await waitFor(() => !!request("session/load"), "session/load");
      await vi.advanceTimersByTimeAsync(60_000);
      await failed;
      expect(request("session/new")).toBeUndefined();
      expect(events.some((event) => event.type === "session.providerBound")).toBe(false);
    } finally {
      vi.useRealTimers();
    }

    sent.length = 0;
    const retry = sendDevinTurn({
      sessionId: "devin-thread",
      cwd: "/repo",
      model: "devin:glm-5-2",
      runtimeMode: "auto",
      text: "continue",
      onEvent: () => undefined,
    });
    await startSession(true);
    expect(request("session/load")!.params.sessionId).toBe("devin-1");
    await answer("session/set_mode", {});
    await answer("session/prompt", { stopReason: "end_turn" });
    await retry;
  });

  it("cancels a permission request that arrives between turns", async () => {
    const events: HarnessEvent[] = [];
    const turn = sendDevinTurn({
      sessionId: "devin-thread",
      cwd: "/repo",
      model: "devin:glm-5-2",
      runtimeMode: "supervised",
      text: "hello",
      onEvent: (event) => events.push(event),
    });
    await startSession();
    await answer("session/set_mode", {});
    await waitFor(() => !!request("session/prompt"), "session/prompt");
    reply(request("session/prompt")!.id, { stopReason: "end_turn" });
    await turn;

    onLine!(
      JSON.stringify({
        jsonrpc: "2.0",
        id: "perm-stray",
        method: "session/request_permission",
        params: {
          sessionId: "devin-1",
          toolCall: { toolCallId: "call-3" },
          options: [
            { optionId: "allow_once", name: "Allow", kind: "allow_once" },
            { optionId: "reject_once", name: "Reject", kind: "reject_once" },
          ],
        },
      }),
    );
    await waitFor(
      () => parse().some((message) => message.id === "perm-stray"),
      "permission reply",
    );
    expect(parse().find((message) => message.id === "perm-stray")!.result).toEqual({
      outcome: { outcome: "cancelled" },
    });
    expect(events.some((event) => event.type === "approval.requested")).toBe(false);
  });
});
