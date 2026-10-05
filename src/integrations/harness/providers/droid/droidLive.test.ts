import { beforeEach, describe, expect, it, vi } from "vitest";

const sent: string[] = [];
let onLine: ((line: string) => void) | undefined;
const spawned: { path: string; args: string[] }[] = [];

vi.mock("../../core/child", () => ({
  resolveDroidBinary: async () => ({ path: "/fake/droid" }),
  spawnChild: async (_id: string, path: string, args: string[]) => {
    spawned.push({ path, args });
  },
  killChild: async () => undefined,
  unwatchChild: () => undefined,
  watchChild: (_id: string, line: (value: string) => void) => {
    onLine = line;
  },
  writeChild: async (_id: string, line: string) => {
    sent.push(line);
  },
}));

const refreshDroidCatalog = vi.fn(async () => undefined);
vi.mock("./droidCatalog", () => ({ refreshDroidCatalog }));

const {
  bindDroidSession,
  respondDroidApproval,
  sendDroidTurn,
  stopDroidSession,
} = await import("./droid");
import type { HarnessEvent } from "../../core/types";

const parse = () => sent.map((line) => JSON.parse(line));

function reply(id: number, result: unknown) {
  onLine!(JSON.stringify({ jsonrpc: "2.0", id, result }));
}

function fail(id: number, error: unknown) {
  onLine!(JSON.stringify({ jsonrpc: "2.0", id, error }));
}

function notify(update: unknown) {
  onLine!(
    JSON.stringify({
      jsonrpc: "2.0",
      method: "session/update",
      params: { sessionId: "droid-session-1", update },
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

type Message = any;

async function next(
  method: string,
  match: (message: Message) => boolean = () => true,
): Promise<Message> {
  let found: Message;
  await waitFor(() => {
    found = parse().find(
      (message) => message.method === method && match(message),
    );
    return found != null;
  }, method);
  return found;
}

function config(model: string, efforts: string[], effort: string) {
  return [
    {
      id: "autonomy_level",
      category: "mode",
      currentValue: "normal",
      options: [
        { value: "normal", name: "Auto (Off)" },
        { value: "spec", name: "Spec" },
        { value: "auto-low", name: "Auto (Low)" },
      ],
    },
    {
      id: "model",
      category: "model",
      currentValue: model,
      options: [
        { value: "gpt-6-luna", name: "GPT-6 Luna" },
        { value: "claude-opus-5-5", name: "Opus 5.5" },
      ],
    },
    {
      id: "reasoning_effort",
      category: "thought_level",
      currentValue: effort,
      options: efforts.map((value) => ({ value, name: value })),
    },
  ];
}

async function start() {
  const init = await next("initialize");
  reply(init.id, { protocolVersion: 1 });
  const created = await next("session/new");
  reply(created.id, {
    sessionId: "droid-session-1",
    models: { currentModelId: "gpt-6-luna", availableModels: [] },
    configOptions: config("gpt-6-luna", ["none", "low", "medium"], "medium"),
  });
}

describe("Factory Droid live ACP sequence", () => {
  beforeEach(() => {
    sent.length = 0;
    spawned.length = 0;
    onLine = undefined;
  });

  it("spawns droid ACP, switches model then effort, sets autonomy, and prompts", async () => {
    const events: HarnessEvent[] = [];
    const turn = sendDroidTurn({
      sessionId: "droid-live-new",
      cwd: "/repo",
      model: "droid:claude-opus-5-5",
      modelSettings: { effort: "xhigh" },
      runtimeMode: "auto-accept-edits",
      text: "inspect this",
      attachments: [],
      onEvent: (event) => events.push(event),
    });

    await start();
    expect(spawned[0]).toEqual({
      path: "/fake/droid",
      args: ["exec", "--output-format", "acp"],
    });

    const setModel = await next(
      "session/set_config_option",
      (message) => message.params.configId === "model",
    );
    expect(setModel.params.value).toBe("claude-opus-5-5");
    // Droid answers with `{}` and announces the new per-model levels.
    notify({
      sessionUpdate: "config_option_update",
      configOptions: config("claude-opus-5-5", ["low", "high", "xhigh", "max"], "high"),
    });
    reply(setModel.id, {});

    const setEffort = await next(
      "session/set_config_option",
      (message) => message.params.configId === "reasoning_effort",
    );
    expect(setEffort.params.value).toBe("xhigh");
    reply(setEffort.id, {});

    const setMode = await next("session/set_mode");
    expect(setMode.params.modeId).toBe("auto-low");
    reply(setMode.id, {});

    const prompt = await next("session/prompt");
    expect(prompt.params).toEqual({
      sessionId: "droid-session-1",
      prompt: [{ type: "text", text: "inspect this" }],
    });
    notify({
      sessionUpdate: "agent_message_chunk",
      content: { type: "text", text: "done" },
    });
    reply(prompt.id, { stopReason: "end_turn" });

    await turn;
    expect(events).toContainEqual({
      type: "session.providerBound",
      providerSessionId: "droid-session-1",
    });
    expect(events).toContainEqual({ type: "message.delta", text: "done" });
    // The first live session seeds the catalog and kicks off the effort probe.
    expect(refreshDroidCatalog).toHaveBeenCalledTimes(1);
    await stopDroidSession("droid-live-new");
  });

  it("loads a bound Droid session instead of creating a new one", async () => {
    bindDroidSession("droid-live-load", "persisted-session", "/repo");
    const turn = sendDroidTurn({
      sessionId: "droid-live-load",
      cwd: "/repo",
      model: "droid:gpt-6-luna",
      runtimeMode: "supervised",
      text: "continue",
      attachments: [],
      onEvent: () => undefined,
    });

    const init = await next("initialize");
    reply(init.id, { protocolVersion: 1 });
    const load = await next("session/load");
    expect(load.params.sessionId).toBe("persisted-session");
    reply(load.id, {
      models: { currentModelId: "gpt-6-luna" },
      configOptions: config("gpt-6-luna", ["low"], "low"),
    });

    const setMode = await next("session/set_mode");
    expect(setMode.params.modeId).toBe("normal");
    reply(setMode.id, {});
    const prompt = await next("session/prompt");
    reply(prompt.id, { stopReason: "end_turn" });

    await turn;
    expect(parse().some((message) => message.method === "session/new")).toBe(false);
    expect(
      parse().some((message) => message.method === "session/set_config_option"),
    ).toBe(false);
    await stopDroidSession("droid-live-load");
  });

  it("asks MonoCode before running a command in supervised mode", async () => {
    const events: HarnessEvent[] = [];
    const turn = sendDroidTurn({
      sessionId: "droid-live-approval",
      cwd: "/repo",
      model: "droid:gpt-6-luna",
      runtimeMode: "supervised",
      text: "run tests",
      attachments: [],
      onEvent: (event) => events.push(event),
    });

    await start();
    const setMode = await next("session/set_mode");
    reply(setMode.id, {});
    const prompt = await next("session/prompt");

    onLine!(
      JSON.stringify({
        jsonrpc: "2.0",
        id: 900,
        method: "session/request_permission",
        params: {
          sessionId: "droid-session-1",
          toolCall: {
            toolCallId: "call-1",
            title: "npm test",
            kind: "execute",
            rawInput: { command: "npm test" },
          },
          options: [
            { optionId: "proceed_once", name: "Allow", kind: "allow_once" },
            { optionId: "cancel", name: "Deny", kind: "reject_once" },
          ],
        },
      }),
    );
    await waitFor(
      () => events.some((event) => event.type === "approval.requested"),
      "approval.requested",
    );
    respondDroidApproval("droid-live-approval", 900, "allow");
    await waitFor(
      () => parse().some((message) => message.id === 900 && message.result),
      "permission reply",
    );
    const answer = parse().find((message) => message.id === 900);
    expect(answer.result.outcome.outcome).toBe("selected");

    reply(prompt.id, { stopReason: "end_turn" });
    await turn;
    await stopDroidSession("droid-live-approval");
  });

  it("reports Droid's hidden error detail once, without the streamed echo", async () => {
    const events: HarnessEvent[] = [];
    const turn = sendDroidTurn({
      sessionId: "droid-live-limit",
      cwd: "/repo",
      model: "droid:gpt-6-luna",
      runtimeMode: "supervised",
      text: "hi",
      attachments: [],
      onEvent: (event) => events.push(event),
    });

    await start();
    const setMode = await next("session/set_mode");
    reply(setMode.id, {});
    const prompt = await next("session/prompt");
    const data =
      '402 {"detail":"You\'ve reached your 5-hour Droid Core usage limit.","status":402}';
    notify({
      sessionUpdate: "agent_message_chunk",
      content: { type: "text", text: `Error: ${data}` },
    });
    fail(prompt.id, { code: -32603, message: "Internal error: Agent error", data });

    await expect(turn).rejects.toThrow();
    expect(events.some((event) => event.type === "message.delta")).toBe(false);
    expect(events).toContainEqual({
      type: "session.error",
      message: "You've reached your 5-hour Droid Core usage limit.",
    });
  });
});
