import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { HarnessEvent, SendTurnInput } from "../../core/types";
import { version } from "../../../../../package.json";

const io = vi.hoisted(() => ({
  lines: new Map<string, (line: string) => void>(),
  exits: new Map<string, (code: number | null) => void>(),
  sent: [] as Array<{
    child: string;
    id?: number | string;
    method?: string;
    params?: any;
    result?: any;
  }>,
  spawn: vi.fn(async () => undefined),
  kill: vi.fn(async () => undefined),
  resolve: vi.fn(async () => ({ path: "/fake/copilot" })),
  holdSetup: false,
  failLoad: false,
  loadSupported: true,
  failMethod: "",
  omitModelNotification: false,
  models: new Map<string, string>(),
  efforts: new Map<string, string>(),
  modes: new Map<string, string>(),
  shortModes: false,
  noEffortModels: new Set<string>(),
}));
vi.mock("../../core/child", () => ({
  resolveCopilotBinary: io.resolve,
  spawnChild: io.spawn,
  killChild: io.kill,
  unwatchChild: (id: string) => {
    io.lines.delete(id);
    io.exits.delete(id);
  },
  watchChild: (
    id: string,
    line: (value: string) => void,
    exit: (code: number | null) => void,
  ) => {
    io.lines.set(id, line);
    io.exits.set(id, exit);
  },
  writeChild: async (child: string, line: string) => {
    const message = JSON.parse(line);
    io.sent.push({ child, ...message });
    if (
      !message.method ||
      message.method === "session/prompt" ||
      message.method === "session/cancel"
    )
      return;
    if (
      io.holdSetup &&
      ["session/new", "session/load"].includes(message.method)
    )
      return;
    if (message.method === io.failMethod) {
      io.lines.get(child)?.(
        JSON.stringify({
          id: message.id,
          error: { code: -32601, message: "Unsupported control" },
        }),
      );
      return;
    }
    if (message.method === "session/set_mode")
      io.modes.set(child, message.params.modeId);
    if (message.method === "session/set_model") {
      io.models.set(child, message.params.modelId);
      io.efforts.delete(child);
    }
    if (
      message.method === "session/set_config_option" &&
      message.params.configId === "reasoning_effort"
    )
      io.efforts.set(child, message.params.value);
    const mode =
      io.modes.get(child) ??
      "https://agentclientprotocol.com/protocol/session-modes#agent";
    // Copilot CLI 1.0.94 reports full mode URIs in configOptions.
    const configOptions = [
      {
        id: "mode",
        currentValue: io.shortModes ? mode.split("#").at(-1)! : mode,
      },
      { id: "model", currentValue: io.models.get(child) ?? "auto" },
      {
        id: "reasoning_effort",
        currentValue: io.efforts.get(child) ?? "medium",
        options: ["low", "medium", "high", "max"].map((value) => ({ value })),
      },
    ].filter(
      (option) =>
        option.id !== "reasoning_effort" ||
        !io.noEffortModels.has(io.models.get(child) ?? "auto"),
    );
    let result: unknown = {};
    if (message.method === "initialize")
      result = { agentCapabilities: { loadSession: io.loadSupported } };
    if (message.method === "session/new")
      result = { sessionId: `provider-${child}`, configOptions };
    if (message.method === "session/set_config_option")
      result = { configOptions };
    if (message.method === "session/set_model" && !io.omitModelNotification) {
      io.lines.get(child)?.(
        JSON.stringify({
          method: "session/update",
          params: {
            update: {
              sessionUpdate: "config_option_update",
              configOptions: configOptions.map((option) =>
                option.id === "model"
                  ? { ...option, currentValue: message.params.modelId }
                  : option,
              ),
            },
          },
        }),
      );
    }
    if (message.method === "session/load") {
      result = { configOptions };
      io.lines.get(child)?.(
        JSON.stringify({
          method: "session/update",
          params: {
            update: {
              sessionUpdate: "agent_message_chunk",
              content: { type: "text", text: "old transcript" },
            },
          },
        }),
      );
      if (io.failLoad) {
        io.lines.get(child)?.(
          JSON.stringify({
            id: message.id,
            error: { code: -32000, message: "Session not found" },
          }),
        );
        return;
      }
    }
    io.lines.get(child)?.(
      JSON.stringify({ jsonrpc: "2.0", id: message.id, result }),
    );
  },
}));
import {
  bindCopilotSession,
  cancelCopilotTurn,
  copilotCommands,
  forgetCopilotSession,
  respondCopilotApproval,
  sendCopilotTurn,
  stopCopilotSession,
} from "./copilot";

let events: HarnessEvent[];
const sessionId = "copilot-test";
function input(overrides: Partial<SendTurnInput> = {}): SendTurnInput {
  return {
    sessionId,
    cwd: "/repo",
    model: "copilot:auto",
    runtimeMode: "supervised",
    text: "hello",
    onEvent: (event) => events.push(event),
    ...overrides,
  };
}
function message(child: string, value: unknown): void {
  io.lines.get(child)!(JSON.stringify(value));
}
function latestPrompt() {
  return io.sent.filter((entry) => entry.method === "session/prompt").at(-1)!;
}
async function waitPrompt(count = 1) {
  await vi.waitFor(() =>
    expect(
      io.sent.filter((entry) => entry.method === "session/prompt"),
    ).toHaveLength(count),
  );
  return latestPrompt();
}
function finish(prompt = latestPrompt()) {
  message(prompt.child, { id: prompt.id, result: { stopReason: "end_turn" } });
}
const options = [
  { optionId: "yes-opaque", kind: "allow_once", name: "Allow" },
  { optionId: "no-opaque", kind: "reject_once", name: "Deny" },
];
function permission(child: string, id: string, kind: string) {
  message(child, {
    jsonrpc: "2.0",
    id,
    method: "session/request_permission",
    params: { toolCall: { toolCallId: id, title: "Test tool", kind }, options },
  });
}

beforeEach(() => {
  events = [];
  io.sent.length = 0;
  io.holdSetup = false;
  io.failLoad = false;
  io.loadSupported = true;
  io.failMethod = "";
  io.omitModelNotification = false;
  io.models.clear();
  io.efforts.clear();
  io.modes.clear();
  io.shortModes = false;
  io.noEffortModels.clear();
  vi.clearAllMocks();
});
afterEach(async () => {
  await forgetCopilotSession(sessionId);
});

describe("Copilot ACP lifecycle", () => {
  it("starts stdio, applies model and mode, streams events and forwards attachments", async () => {
    const accepted = vi.fn();
    const turn = sendCopilotTurn(
      input({
        model: "copilot:gpt-4.1",
        onAccepted: accepted,
        attachments: [
          {
            id: "image",
            name: "screen.png",
            mimeType: "image/png",
            kind: "image",
            size: 4,
            data: "AAAA",
          },
        ],
      }),
    );
    const prompt = await waitPrompt();
    expect(io.spawn).toHaveBeenCalledWith(
      prompt.child,
      "/fake/copilot",
      ["--acp", "--stdio", "--no-auto-update"],
      "/repo",
      undefined,
      "copilot",
    );
    expect(
      io.sent.find((entry) => entry.method === "session/set_model")?.params
        .modelId,
    ).toBe("gpt-4.1");
    expect(
      io.sent.find((entry) => entry.method === "initialize")?.params,
    ).toEqual({
      protocolVersion: 1,
      clientCapabilities: {
        fs: { readTextFile: false, writeTextFile: false },
        terminal: false,
      },
      clientInfo: { name: "monocode", version },
    });
    expect(prompt.params.prompt).toEqual([
      { type: "text", text: "hello" },
      { type: "image", mimeType: "image/png", data: "AAAA" },
    ]);
    message(prompt.child, {
      method: "session/update",
      params: {
        update: {
          sessionUpdate: "agent_message_chunk",
          content: { type: "text", text: "answer" },
        },
      },
    });
    message(prompt.child, {
      method: "session/update",
      params: {
        update: {
          sessionUpdate: "agent_thought_chunk",
          content: { type: "text", text: "thinking" },
        },
      },
    });
    finish(prompt);
    await turn;
    expect(accepted).toHaveBeenCalledOnce();
    expect(events).toContainEqual({ type: "message.delta", text: "answer" });
    expect(events).toContainEqual({
      type: "reasoning.delta",
      text: "thinking",
    });
    expect(events).toContainEqual({ type: "message.completed" });
  });

  it("resumes after parking without replaying saved assistant messages", async () => {
    const first = sendCopilotTurn(input());
    finish(await waitPrompt());
    await first;
    const bound = events.find(
      (event) => event.type === "session.providerBound",
    ) as Extract<HarnessEvent, { type: "session.providerBound" }>;
    await stopCopilotSession(sessionId);
    const second = sendCopilotTurn(input());
    finish(await waitPrompt(2));
    await second;
    expect(
      io.sent.find((entry) => entry.method === "session/load")?.params
        .sessionId,
    ).toBe(bound.providerSessionId);
    expect(events).not.toContainEqual({
      type: "message.delta",
      text: "old transcript",
    });
    expect(io.spawn).toHaveBeenCalledTimes(2);
  });

  it("fails a broken resume without silently discarding the conversation", async () => {
    bindCopilotSession(sessionId, "saved-session", "/repo");
    io.failLoad = true;
    await expect(sendCopilotTurn(input())).rejects.toThrow("Session not found");
    expect(io.sent.some((entry) => entry.method === "session/new")).toBe(false);
    expect(io.kill).toHaveBeenCalledOnce();
  });

  it.each([
    "session/set_config_option",
    "session/set_mode",
    "session/set_model",
  ])(
    "fails closed and cleans up when %s rejects a requested change",
    async (method) => {
      io.failMethod = method;
      await expect(
        sendCopilotTurn(input({ model: "copilot:gpt-4.1", intent: "plan" })),
      ).rejects.toThrow("Unsupported control");
      expect(io.kill).toHaveBeenCalledOnce();
      expect(io.lines.size).toBe(0);
      expect(io.sent.some((entry) => entry.method === "session/prompt")).toBe(
        false,
      );
    },
  );

  it("reuses applied mode/model and changes effort without respawning or losing the session", async () => {
    for (const [index, effort] of ["high", "low", undefined].entries()) {
      const turn = sendCopilotTurn(
        input({
          model: "copilot:gpt-4.1",
          modelSettings: effort ? { reasoningEffort: effort } : undefined,
        }),
      );
      finish(await waitPrompt(index + 1));
      await turn;
    }
    expect(io.spawn).toHaveBeenCalledOnce();
    expect(
      io.sent.filter((entry) => entry.method === "session/set_mode"),
    ).toHaveLength(0);
    expect(
      io.sent.filter((entry) => entry.method === "session/set_model"),
    ).toHaveLength(1);
    const controls = io.sent.filter(
      (entry) => entry.method === "session/set_config_option",
    );
    expect(
      controls.filter((entry) => entry.params.configId === "allow_all"),
    ).toHaveLength(3);
    expect(
      controls
        .filter((entry) => entry.params.configId === "reasoning_effort")
        .map((entry) => entry.params.value),
    ).toEqual(["high", "low", "medium"]);
    expect(
      new Set(
        io.sent
          .filter((entry) => entry.method === "session/prompt")
          .map((entry) => entry.params.sessionId),
      ).size,
    ).toBe(1);
  });

  it("uses the fresh config response when model notifications arrive late", async () => {
    io.omitModelNotification = true;
    const turn = sendCopilotTurn(
      input({
        model: "copilot:gpt-4.1",
        modelSettings: { reasoningEffort: "high" },
      }),
    );
    finish(await waitPrompt());
    await turn;
    expect(
      io.sent
        .filter((entry) => entry.method === "session/set_config_option")
        .map((entry) => entry.params.configId),
    ).toEqual(["allow_all", "reasoning_effort"]);
  });

  it("rejects invalid effort without discarding the child or conversation", async () => {
    await expect(
      sendCopilotTurn(input({ modelSettings: { reasoningEffort: "none" } })),
    ).rejects.toThrow('does not support reasoning effort "none"');
    expect(io.kill).not.toHaveBeenCalled();
    expect(io.sent.some((entry) => entry.method === "session/prompt")).toBe(
      false,
    );
    const retry = sendCopilotTurn(
      input({ modelSettings: { reasoningEffort: "high" } }),
    );
    finish(await waitPrompt());
    await retry;
    expect(io.spawn).toHaveBeenCalledOnce();
    expect(
      io.sent.filter((entry) => entry.method === "session/new"),
    ).toHaveLength(1);
  });

  it("ignores stale effort after switching to a model without an effort option", async () => {
    const first = sendCopilotTurn(
      input({
        model: "copilot:gpt-4.1",
        modelSettings: { reasoningEffort: "high" },
      }),
    );
    finish(await waitPrompt());
    await first;
    io.noEffortModels.add("no-effort-model");
    const before = io.sent.length;
    const next = sendCopilotTurn(
      input({
        model: "copilot:no-effort-model",
        modelSettings: { reasoningEffort: "high" },
      }),
    );
    finish(await waitPrompt(2));
    await next;
    expect(
      io.sent
        .slice(before)
        .some(
          (entry) =>
            entry.method === "session/set_config_option" &&
            entry.params.configId === "reasoning_effort",
        ),
    ).toBe(false);
    expect(io.spawn).toHaveBeenCalledOnce();
    expect(io.kill).not.toHaveBeenCalled();
  });

  it.each([false, true])(
    "caches full and shorthand mode ids (short=%s)",
    async (short) => {
      io.shortModes = short;
      for (const [index, intent] of [
        undefined,
        undefined,
        "plan",
        "plan",
        undefined,
      ].entries()) {
        const turn = sendCopilotTurn(
          input({ intent: intent as SendTurnInput["intent"] }),
        );
        finish(await waitPrompt(index + 1));
        await turn;
      }
      expect(
        io.sent
          .filter((entry) => entry.method === "session/set_mode")
          .map((entry) => entry.params.modeId),
      ).toEqual([
        "https://agentclientprotocol.com/protocol/session-modes#plan",
        "https://agentclientprotocol.com/protocol/session-modes#agent",
      ]);
    },
  );

  it("fails resume when the CLI does not advertise loadSession", async () => {
    bindCopilotSession(sessionId, "saved-session", "/repo");
    io.loadSupported = false;
    await expect(sendCopilotTurn(input())).rejects.toThrow("cannot resume");
    expect(
      io.sent.some(
        (entry) =>
          entry.method === "session/new" || entry.method === "session/load",
      ),
    ).toBe(false);
    expect(io.kill).toHaveBeenCalledOnce();
  });

  it("denies pending approvals on teardown", async () => {
    const turn = sendCopilotTurn(input());
    const prompt = await waitPrompt();
    permission(prompt.child, "pending-approval", "execute");
    await vi.waitFor(() =>
      expect(events.some((event) => event.type === "approval.requested")).toBe(
        true,
      ),
    );
    await cancelCopilotTurn(sessionId);
    await turn;
    expect(events).toContainEqual(
      expect.objectContaining({ type: "approval.resolved", decision: "deny" }),
    );
    expect(
      io.sent.find((entry) => entry.id === "pending-approval")?.result.outcome
        .optionId,
    ).toBe("no-opaque");
  });

  it("denies permissions while session setup is muted even in full-access mode", async () => {
    io.holdSetup = true;
    const turn = sendCopilotTurn(input({ runtimeMode: "full-access" }));
    await vi.waitFor(() =>
      expect(io.sent.some((entry) => entry.method === "session/new")).toBe(
        true,
      ),
    );
    const setup = io.sent.find((entry) => entry.method === "session/new")!;
    permission(setup.child, "muted-permission", "execute");
    await vi.waitFor(() =>
      expect(
        io.sent.find((entry) => entry.id === "muted-permission")?.result.outcome
          .optionId,
      ).toBe("no-opaque"),
    );
    expect(events.some((event) => event.type === "approval.requested")).toBe(
      false,
    );
    await cancelCopilotTurn(sessionId);
    await turn;
  });

  it("serializes turns without changing the active listener or policy", async () => {
    const secondEvents: HarnessEvent[] = [];
    const first = sendCopilotTurn(input());
    const firstPrompt = await waitPrompt();
    const second = sendCopilotTurn(
      input({
        runtimeMode: "full-access",
        onEvent: (event) => secondEvents.push(event),
      }),
    );
    permission(firstPrompt.child, "permission-uuid", "execute");
    await vi.waitFor(() =>
      expect(events.some((event) => event.type === "approval.requested")).toBe(
        true,
      ),
    );
    expect(secondEvents).toEqual([]);
    const approval = events.find(
      (event) => event.type === "approval.requested",
    ) as Extract<HarnessEvent, { type: "approval.requested" }>;
    respondCopilotApproval(sessionId, approval.requestId, "allow");
    await vi.waitFor(() =>
      expect(
        io.sent.find((entry) => entry.id === "permission-uuid")?.result.outcome
          .optionId,
      ).toBe("yes-opaque"),
    );
    finish(firstPrompt);
    await first;
    finish(await waitPrompt(2));
    await second;
    expect(io.spawn).toHaveBeenCalledOnce();
  });

  it.each([
    ["auto-accept-edits", undefined, "edit", "yes-opaque"],
    ["full-access", undefined, "execute", "yes-opaque"],
    ["full-access", "plan", "read", "yes-opaque"],
    ["full-access", "plan", "edit", "no-opaque"],
    ["full-access", "plan", "execute", "no-opaque"],
  ] as const)(
    "enforces %s/%s permission policy for %s",
    async (runtimeMode, intent, kind, expected) => {
      const turn = sendCopilotTurn(input({ runtimeMode, intent }));
      const prompt = await waitPrompt();
      permission(prompt.child, "server-request", kind);
      await vi.waitFor(() =>
        expect(
          io.sent.find((entry) => entry.id === "server-request")?.result.outcome
            .optionId,
        ).toBe(expected),
      );
      expect(events.some((event) => event.type === "approval.requested")).toBe(
        false,
      );
      const mode = io.sent.find((entry) => entry.method === "session/set_mode")
        ?.params.modeId;
      if (intent === "plan") expect(mode).toMatch(/#plan$/);
      else expect(mode).toBeUndefined();
      finish(prompt);
      await turn;
    },
  );

  it("cancels the active and queued turns, then resumes on a fresh child", async () => {
    const first = sendCopilotTurn(input());
    const old = await waitPrompt();
    const second = sendCopilotTurn(input());
    await cancelCopilotTurn(sessionId);
    await Promise.all([first, second]);
    expect(
      io.sent.filter((entry) => entry.method === "session/prompt"),
    ).toHaveLength(1);
    const third = sendCopilotTurn(input());
    const next = await waitPrompt(2);
    expect(next.child).not.toBe(old.child);
    finish(next);
    await third;
  });

  it("cleans up an unexpected exit and resumes the saved provider session", async () => {
    const first = sendCopilotTurn(input());
    const failed = await waitPrompt();
    const rejected = expect(first).rejects.toThrow("Copilot CLI exited");
    io.exits.get(failed.child)?.(1);
    await rejected;
    expect(io.lines.has(failed.child)).toBe(false);
    const next = sendCopilotTurn(input());
    finish(await waitPrompt(2));
    await next;
    expect(io.sent.some((entry) => entry.method === "session/load")).toBe(true);
  });

  it("cancels a pending session setup without waiting for its timeout", async () => {
    io.holdSetup = true;
    const turn = sendCopilotTurn(input());
    await vi.waitFor(() =>
      expect(io.sent.some((entry) => entry.method === "session/new")).toBe(
        true,
      ),
    );
    await cancelCopilotTurn(sessionId);
    await turn;
    expect(io.sent.some((entry) => entry.method === "session/prompt")).toBe(
      false,
    );
    expect(io.kill).toHaveBeenCalledOnce();
  });

  it("kills a child whose spawn completes after cancellation", async () => {
    let spawned!: () => void;
    io.spawn.mockImplementationOnce(
      () =>
        new Promise<void>((resolve) => {
          spawned = resolve;
        }),
    );
    const turn = sendCopilotTurn(input());
    await vi.waitFor(() => expect(io.spawn).toHaveBeenCalledOnce());
    await cancelCopilotTurn(sessionId);
    spawned();
    await turn;
    expect(io.kill).toHaveBeenCalledTimes(2);
    expect(io.sent.some((entry) => entry.method === "initialize")).toBe(false);
    expect(io.lines.size).toBe(0);
  });

  it("does not spawn after cancellation during binary resolution", async () => {
    let resolve!: (value: { path: string }) => void;
    io.resolve.mockImplementationOnce(
      () =>
        new Promise((done) => {
          resolve = done;
        }),
    );
    const turn = sendCopilotTurn(input());
    await vi.waitFor(() => expect(io.resolve).toHaveBeenCalledOnce());
    await cancelCopilotTurn(sessionId);
    resolve({ path: "/fake/copilot" });
    await turn;
    expect(io.spawn).not.toHaveBeenCalled();
  });

  it("uses advertised commands and strips only the reserved namespace", async () => {
    const first = sendCopilotTurn(input());
    const prompt = await waitPrompt();
    message(prompt.child, {
      method: "session/update",
      params: {
        update: {
          sessionUpdate: "available_commands_update",
          availableCommands: [
            { name: "compact", description: "Compact" },
            { name: "context", description: "Context" },
          ],
        },
      },
    });
    expect(
      copilotCommands(sessionId).map((command) => command.invocation),
    ).toEqual(["copilot:compact", "context"]);
    finish(prompt);
    await first;
    const second = sendCopilotTurn(
      input({ text: "/copilot:compact focus on tests" }),
    );
    const next = await waitPrompt(2);
    expect(next.params.prompt).toEqual([
      { type: "text", text: "/compact focus on tests" },
    ]);
    finish(next);
    await second;
  });
});
