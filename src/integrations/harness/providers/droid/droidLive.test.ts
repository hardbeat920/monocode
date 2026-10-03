import { beforeEach, describe, expect, it, vi } from "vitest";

const sent: string[] = [];
let onLine: ((line: string) => void) | undefined;
let onExit: ((code: number) => void) | undefined;
let resolveGate: Promise<void> | undefined;
let killed = false;
let killGate: Promise<void> | undefined;
let permissionWriteGate: Promise<void> | undefined;
const spawned: { path: string; args: string[] }[] = [];
const headless = vi.hoisted(() => ({ enabled: false }));

vi.mock("../../core/child", () => ({
  hasHeadlessChildBackend: () => headless.enabled,
  resolveDroidBinary: async () => {
    await resolveGate;
    return { path: "/fake/droid" };
  },
  spawnChild: async (_id: string, path: string, args: string[]) => {
    spawned.push({ path, args });
    killed = false;
  },
  killChild: async () => {
    killed = true;
    await killGate;
  },
  unwatchChild: () => undefined,
  watchChild: (
    _id: string,
    line: (value: string) => void,
    exit: (code: number) => void,
  ) => {
    onLine = line;
    onExit = exit;
  },
  writeChild: async (_id: string, line: string) => {
    if (permissionWriteGate && JSON.parse(line).result?.outcome)
      await permissionWriteGate;
    if (killed) throw new Error("Harness process not running");
    sent.push(line);
  },
}));

const refreshDroidCatalog = vi.fn(async () => undefined);
vi.mock("./droidCatalog", () => ({ refreshDroidCatalog }));

const {
  bindDroidSession,
  cancelDroidTurn,
  forgetDroidSession,
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
    headless.enabled = false;
    refreshDroidCatalog.mockClear();
    sent.length = 0;
    spawned.length = 0;
    onLine = undefined;
    onExit = undefined;
    resolveGate = undefined;
    killed = false;
    killGate = undefined;
    permissionWriteGate = undefined;
  });

  it.each([false, true])(
    "spawns droid ACP, switches model then effort, sets autonomy, and prompts with headless %s",
    async (isHeadless) => {
      headless.enabled = isHeadless;
      const sessionId = `droid-live-new-${isHeadless}`;
      const events: HarnessEvent[] = [];
      const turn = sendDroidTurn({
        sessionId,
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
        configOptions: config(
          "claude-opus-5-5",
          ["low", "high", "xhigh", "max"],
          "high",
        ),
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
      expect(refreshDroidCatalog).toHaveBeenCalledTimes(isHeadless ? 0 : 1);
      await forgetDroidSession(sessionId);
    },
  );

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
    expect(parse().some((message) => message.method === "session/new")).toBe(
      false,
    );
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
    expect(answer.result.outcome).toEqual({
      outcome: "selected",
      optionId: "proceed_once",
    });

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
    fail(prompt.id, {
      code: -32603,
      message: "Internal error: Agent error",
      data,
    });

    await expect(turn).rejects.toThrow();
    expect(events.some((event) => event.type === "message.delta")).toBe(false);
    expect(events).toContainEqual({
      type: "session.error",
      message: "You've reached your 5-hour Droid Core usage limit.",
    });
  });
});

const input = (
  sessionId: string,
  onEvent: (event: HarnessEvent) => void = () => {},
) => ({
  sessionId,
  cwd: "/repo",
  model: "droid:gpt-6-luna",
  runtimeMode: "supervised" as const,
  text: "test",
  attachments: [],
  onEvent,
});
const options = [
  { optionId: "proceed_once", kind: "allow_once", name: "Allow" },
  { optionId: "cancel", kind: "reject_once", name: "Deny" },
];
function permission(id: number | string, kind?: string) {
  onLine!(
    JSON.stringify({
      jsonrpc: "2.0",
      id,
      method: "session/request_permission",
      params: {
        toolCall: { toolCallId: "call-1", title: "test", kind },
        options,
      },
    }),
  );
}
async function ready(
  sessionId: string,
  onEvent?: (event: HarnessEvent) => void,
) {
  const turn = sendDroidTurn(input(sessionId, onEvent));
  const settled = turn.then(
    () => undefined,
    (error) => error,
  );
  await start();
  const mode = await next("session/set_mode");
  reply(mode.id, {});
  const prompt = await next("session/prompt");
  return { turn, settled, prompt };
}

describe("Droid issue 1 regressions", () => {
  it("drains a permission reply when its tool update stops the session", async () => {
    const sessionId = "stop-tool-update-drain";
    let stopping: Promise<void> | undefined;
    const { turn } = await ready(sessionId, (event) => {
      if (event.type === "tool.updated") stopping = stopDroidSession(sessionId);
    });
    let release!: () => void;
    permissionWriteGate = new Promise<void>((resolve) => {
      release = resolve;
    });
    permission(907, "execute");
    await new Promise((resolve) => setTimeout(resolve, 20));
    const stoppedBeforeReply = killed;
    release();
    await stopping;
    await turn;
    expect(stoppedBeforeReply).toBe(false);
    expect(
      parse().find((message) => message.id === 907)?.result?.outcome,
    ).toEqual({ outcome: "cancelled" });
    expect(killed).toBe(true);
  });

  it.each(["supervised", "full-access"] as const)(
    "cancels permission decisions if a tool update stops %s mode",
    async (runtimeMode) => {
      const sessionId = `cancel-tool-update-${runtimeMode}`;
      const events: HarnessEvent[] = [];
      let stopping: Promise<void> | undefined;
      const turn = sendDroidTurn({
        ...input(sessionId, (event) => {
          events.push(event);
          if (event.type === "tool.updated")
            stopping = cancelDroidTurn(sessionId);
          // Exit on an unexpected late approval so a failed assertion does not
          // leave a live prompt.
          if (event.type === "approval.requested") onExit!(1);
        }),
        runtimeMode,
      });
      await start();
      const mode = await next("session/set_mode");
      reply(mode.id, {});
      await next("session/prompt");
      permission(906, "execute");
      await stopping;
      await turn;
      expect(events.some((event) => event.type === "approval.requested")).toBe(
        false,
      );
      expect(
        parse().find((message) => message.id === 906)?.result?.outcome,
      ).toEqual({ outcome: "cancelled" });
    },
  );

  beforeEach(() => {
    sent.length = 0;
    spawned.length = 0;
    onLine = undefined;
    onExit = undefined;
    resolveGate = undefined;
    killed = false;
    killGate = undefined;
    permissionWriteGate = undefined;
  });

  it("invalidates startup when a session is forgotten", async () => {
    let release!: () => void;
    resolveGate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const events: HarnessEvent[] = [];
    const turn = sendDroidTurn(input("deleted", (event) => events.push(event)));
    const settled = turn.catch(() => undefined);
    await cancelDroidTurn("deleted");
    await forgetDroidSession("deleted");
    release();
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(spawned).toEqual([]);
    expect(events).toEqual([]);
    await settled;
  });

  it("rejects an unexpected process exit", async () => {
    const events: HarnessEvent[] = [];
    const { settled } = await ready("crashed", (event) => events.push(event));
    onExit!(1);
    expect(await settled).toBeInstanceOf(Error);
    expect(events.some((event) => event.type === "session.error")).toBe(true);
  });

  it("registers approval before synchronous denial and preserves string identifiers", async () => {
    const { turn, prompt } = await ready("sync", (event) => {
      if (event.type === "approval.requested")
        respondDroidApproval("sync", event.requestId, "deny");
    });
    permission("permission-abc", "execute");
    await waitFor(
      () =>
        parse().some(
          (message) => message.id === "permission-abc" && message.result,
        ),
      "string reply",
    );
    expect(
      parse().find((message) => message.id === "permission-abc").result.outcome,
    ).toEqual({ outcome: "selected", optionId: "cancel" });
    reply(prompt.id, {});
    await turn;
    await stopDroidSession("sync");
  });

  it("keeps sparse execute permissions supervised in edit mode", async () => {
    const events: HarnessEvent[] = [];
    const turn = sendDroidTurn({
      ...input("sparse", (event) => events.push(event)),
      runtimeMode: "auto-accept-edits",
    });
    await start();
    const mode = await next("session/set_mode");
    reply(mode.id, {});
    const prompt = await next("session/prompt");
    notify({
      sessionUpdate: "tool_call",
      toolCallId: "call-1",
      kind: "execute",
      title: "test",
    });
    permission(901);
    await waitFor(
      () => events.some((event) => event.type === "approval.requested"),
      "sparse approval",
    );
    expect(
      events.find((event) => event.type === "approval.requested"),
    ).toMatchObject({ kind: "execute" });
    respondDroidApproval("sparse", 901, "deny");
    reply(prompt.id, {});
    await turn;
    await stopDroidSession("sparse");
  });

  it("cancels pending and late permissions without approval or unsafe writes", async () => {
    const events: HarnessEvent[] = [];
    const { turn } = await ready("cancel-approval", (event) =>
      events.push(event),
    );
    permission(902, "execute");
    await waitFor(
      () => events.some((event) => event.type === "approval.requested"),
      "approval",
    );
    await cancelDroidTurn("cancel-approval");
    await turn;
    const answer = parse().find(
      (message) => message.id === 902 && message.result,
    );
    expect(answer.result.outcome).toEqual({ outcome: "cancelled" });
    const count = events.filter(
      (event) => event.type === "approval.requested",
    ).length;
    permission(903, "edit");
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(
      events.filter((event) => event.type === "approval.requested"),
    ).toHaveLength(count);
    expect(killed).toBe(true);
  });

  it("preserves the binding after a transient resume error", async () => {
    bindDroidSession("resume-error", "retained", "/repo");
    const turn = sendDroidTurn(input("resume-error"));
    const settled = turn.then(
      () => undefined,
      (error) => error,
    );
    const init = await next("initialize");
    reply(init.id, {});
    const load = await next("session/load");
    fail(load.id, { code: -32603, message: "Temporary storage failure" });
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(parse().some((message) => message.method === "session/new")).toBe(
      false,
    );
    expect(await settled).toBeInstanceOf(Error);
  });

  it("waits for delayed model config before applying effort", async () => {
    const turn = sendDroidTurn({
      ...input("delayed-config"),
      model: "droid:claude-opus-5-5",
      modelSettings: { effort: "xhigh" },
    });
    await start();
    const model = await next("session/set_config_option");
    reply(model.id, {});
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(parse().some((message) => message.method === "session/prompt")).toBe(
      false,
    );
    notify({
      sessionUpdate: "config_option_update",
      configOptions: config("claude-opus-5-5", ["high", "xhigh"], "high"),
    });
    const effort = await next(
      "session/set_config_option",
      (message) => message.params.configId === "reasoning_effort",
    );
    expect(effort.params.value).toBe("xhigh");
    reply(effort.id, {});
    const prompt = await next("session/prompt");
    reply(prompt.id, {});
    await turn;
    await stopDroidSession("delayed-config");
  });

  it("retains a mode notification received before the control reply", async () => {
    const turn = sendDroidTurn(input("mode-notification"));
    await start();
    const mode = await next("session/set_mode");
    notify({ sessionUpdate: "current_mode_update", currentModeId: "auto-low" });
    reply(mode.id, {});
    const prompt = await next("session/prompt");
    reply(prompt.id, {});
    await turn;

    const second = sendDroidTurn(input("mode-notification"));
    const restore = await next(
      "session/set_mode",
      (message) => message.id !== mode.id,
    );
    expect(restore.params.modeId).toBe("normal");
    reply(restore.id, {});
    const secondPrompt = await next(
      "session/prompt",
      (message) => message.id !== prompt.id,
    );
    reply(secondPrompt.id, {});
    await second;
    await stopDroidSession("mode-notification");
  });

  it("switches back after an agent model fallback", async () => {
    const { turn, prompt } = await ready("fallback");
    reply(prompt.id, {});
    await turn;
    notify({
      sessionUpdate: "config_option_update",
      configOptions: config("fallback", ["low"], "low"),
    });
    const second = sendDroidTurn(input("fallback"));
    const model = await next(
      "session/set_config_option",
      (message) => message.params.configId === "model",
    );
    expect(model.params.value).toBe("gpt-6-luna");
    reply(model.id, {});
    const secondPrompt = await next(
      "session/prompt",
      (message) => message.id !== prompt.id,
    );
    reply(secondPrompt.id, {});
    await second;
    await stopDroidSession("fallback");
  });

  it("queues a follow-up until cancellation finishes retiring the child", async () => {
    const { turn } = await ready("retirement");
    const oldLine = onLine!;
    let release!: () => void;
    killGate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const cancelling = cancelDroidTurn("retirement");
    const events: HarnessEvent[] = [];
    const following = sendDroidTurn(
      input("retirement", (event) => events.push(event)),
    );
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(spawned).toHaveLength(1);
    release();
    killGate = undefined;
    await cancelling;
    await turn;
    await waitFor(
      () =>
        parse().filter((message) => message.method === "initialize").length ===
        2,
      "new connection",
    );
    reply(
      parse().filter((message) => message.method === "initialize")[1].id,
      {},
    );
    const load = await next("session/load");
    reply(load.id, {
      models: { currentModelId: "gpt-6-luna" },
      configOptions: config("gpt-6-luna", ["low", "medium"], "medium"),
    });
    await waitFor(
      () =>
        parse().filter((message) => message.method === "session/set_mode")
          .length === 2,
      "new mode",
    );
    reply(
      parse().filter((message) => message.method === "session/set_mode")[1].id,
      {},
    );
    await waitFor(
      () =>
        parse().filter((message) => message.method === "session/prompt")
          .length === 2,
      "new prompt",
    );
    oldLine(
      JSON.stringify({
        method: "session/update",
        params: {
          update: {
            sessionUpdate: "agent_message_chunk",
            content: { type: "text", text: "old tail" },
          },
        },
      }),
    );
    expect(events.some((event) => event.type === "message.delta")).toBe(false);
    reply(
      parse().filter((message) => message.method === "session/prompt")[1].id,
      {},
    );
    await following;
    await stopDroidSession("retirement");
  });

  it("stops with a pending permission without an unhandled write failure", async () => {
    const events: HarnessEvent[] = [];
    const { turn } = await ready("stop-pending", (event) => events.push(event));
    permission(904, "execute");
    await waitFor(
      () => events.some((event) => event.type === "approval.requested"),
      "approval",
    );
    await stopDroidSession("stop-pending");
    await turn;
    expect(
      parse().find((message) => message.id === 904).result.outcome,
    ).toEqual({ outcome: "cancelled" });
    expect(killed).toBe(true);
  });

  it.each(["automatic", "planning"])(
    "uses offered semantic option identifiers for %s permission replies",
    async (mode) => {
      const turn = sendDroidTurn({
        ...input(`semantic-${mode}`),
        runtimeMode: mode === "automatic" ? "full-access" : "supervised",
        intent: mode === "planning" ? "plan" : undefined,
      });
      await start();
      const autonomy = await next("session/set_mode");
      reply(autonomy.id, {});
      const prompt = await next("session/prompt");
      permission(905, mode === "planning" ? "switch_mode" : "execute");
      await waitFor(
        () => parse().some((message) => message.id === 905 && message.result),
        "semantic reply",
      );
      expect(
        parse().find((message) => message.id === 905).result.outcome,
      ).toEqual({
        outcome: "selected",
        optionId: mode === "planning" ? "cancel" : "proceed_once",
      });
      reply(prompt.id, {});
      await turn;
      await stopDroidSession(`semantic-${mode}`);
    },
  );

  it("reports a rejected reasoning effort without dispatching a prompt", async () => {
    const turn = sendDroidTurn({
      ...input("effort-rejected"),
      modelSettings: { effort: "low" },
    });
    const settled = turn.then(
      () => undefined,
      (error) => error,
    );
    await start();
    const effort = await next("session/set_config_option");
    fail(effort.id, { code: -32602, message: "Effort rejected" });
    expect(await settled).toBeInstanceOf(Error);
    expect(parse().some((message) => message.method === "session/prompt")).toBe(
      false,
    );
  });
});
